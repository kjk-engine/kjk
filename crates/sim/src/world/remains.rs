//! The dead's remains, as KOTOR's engine leaves them (both games; read from
//! their executables, behaviour only). A creature dying outside the party
//! is taken away a while after (its appearance's `destroyobjectdelay`,
//! `DESTROY_MS` where blank) unless a script keeps it (`SetIsDestroyable(FALSE)`): its
//! body fades out as it goes (`fadedelayondeath`; the engine's own fade
//! where blank) and, if it held anything that drops, any gold, or its bag
//! is a corpse, its remains stand where it lay, facing its way, once the
//! fade is over (half a second at the least). They are a placeable with no
//! tag, named as their `bodybag.2da` row (strref 38151, "Remains"), drawn
//! as the row's placeable appearance, holding the droppable items it wore
//! and carried and its gold as one stack of credits (`g_i_credits001`).
//! They open as any container does (`containers`, the loot window); closed
//! empty they go, unless they are a corpse (the rancor's, the krayt
//! dragon's).
//!
//! The row: the creature's template's `BodyBag`, else, where that row has
//! no appearance (row 0 "default": every template of both games), its
//! appearance's `body_bag`; a row with no appearance still is a backpack
//! (placeable appearance 3). Neither engine reads `Lootable`, `NoPermDeath`
//! or `DecayTime`. TSL also takes the gear off the dying body's model; K1
//! leaves it on, as here.

use std::sync::Arc;

use script_ncs::vm::ObjectId;

use crate::inventory::Held;
use crate::module::{ModuleData, ObjectData, ObjectKind};
use crate::script::{Change, Made, Out, State, Thing, View};
use crate::wire::{DecodeError, Reader, Writer};

/// How long after its death a creature whose appearance gives no delay is
/// taken away (ms).
pub const DESTROY_MS: u32 = rules::items::remains::DESTROY_MS;
/// How long a body whose appearance gives no fade fades out as it goes
/// (ms) [verify: the engine's own fade's length].
pub const FADE_MS: u32 = rules::items::remains::FADE_MS;
/// The least time from a body being taken away to its remains standing
/// (ms): the engine adds them that much later, shown once the fade is over.
pub const BAG_AFTER_MS: u32 = rules::items::remains::BAG_AFTER_MS;
/// The placeable appearance of remains whose row has none: a backpack.
pub const BACKPACK: u16 = 3;
/// The item the gold of the dead is left as.
pub const CREDITS: &str = "g_i_credits001";

/// The template remains of `bodybag.2da` row `row` are made from.
pub fn bag_template(row: u8) -> String {
    format!("{BAG_PREFIX}{row}")
}

/// The `bodybag.2da` row of remains made from `template`, if they are.
pub fn bag_row(template: &str) -> Option<u8> {
    template.strip_prefix(BAG_PREFIX)?.parse().ok()
}

const BAG_PREFIX: &str = "*bag";

/// A `bodybag.2da` row: its placeable appearance (`None` for blank), its
/// name as the game shows it, and whether it is a corpse (kept emptied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BagRow {
    pub appearance: Option<u16>,
    pub name: String,
    pub corpse: bool,
}

impl BagRow {
    /// Its `placeables.2da` row.
    pub fn placeable(&self) -> u16 {
        self.appearance.unwrap_or(BACKPACK)
    }
}

/// A row past the table's end: a nameless backpack.
const NAMELESS: BagRow = BagRow {
    appearance: None,
    name: String::new(),
    corpse: false,
};

/// `module`'s `bodybag.2da` row `row`.
fn bag_in(module: &ModuleData, row: u8) -> BagRow {
    module
        .bags
        .get(usize::from(row))
        .cloned()
        .unwrap_or(NAMELESS)
}

/// `module`'s bag row of remains made from `template`, if they are.
pub fn bag_of(module: &ModuleData, template: &str) -> Option<BagRow> {
    Some(bag_in(module, bag_row(template)?))
}

/// The facts of remains made from `template` in `module`, standing
/// nowhere and holding nothing yet.
pub(crate) fn bag_facts(module: &ModuleData, template: &str) -> Option<ObjectData> {
    let row = bag_of(module, template)?;
    Some(ObjectData {
        kind: ObjectKind::Placeable,
        name: row.name,
        container: true,
        useable: true,
        ..ObjectData::unknown(template, [0.0; 3], 0.0)
    })
}

/// What remains hold as made (`snapshot::Spawned::holds`): each item
/// carried, droppable.
pub fn held(holds: &[(String, u16)]) -> Vec<Held> {
    holds
        .iter()
        .map(|(template, stack)| Held {
            template: template.clone(),
            stack: *stack,
            slot: None,
            droppable: true,
            infinite: false,
        })
        .collect()
}

/// What `facts` hold as their own (remains'), for `snapshot::Spawned`;
/// none for the rest, which hold their template's.
pub(crate) fn holds(facts: &ObjectData) -> Vec<(String, u16)> {
    if bag_row(&facts.template).is_none() {
        return Vec::new();
    }
    facts
        .inventory
        .iter()
        .map(|held| (held.template.clone(), held.stack))
        .collect()
}

/// A dead creature the engine will take away: when, and, once its body is
/// taken (fading), its remains to come then.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corpse {
    pub due_ms: u64,
    pub bag: Option<Bag>,
}

/// Remains to come: their `bodybag.2da` row, where they stand (metres, on
/// the floor) and which way they face (degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bag {
    pub row: u8,
    pub at: [f32; 3],
    pub yaw: f32,
}

impl Corpse {
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u64(self.due_ms);
        match &self.bag {
            Some(bag) => {
                out.u8(1);
                out.u8(bag.row);
                for value in bag.at {
                    out.f32(value);
                }
                out.f32(bag.yaw);
            }
            None => out.u8(0),
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Corpse, DecodeError> {
        let due_ms = input.u64()?;
        let at = input.at();
        let bag = match input.u8()? {
            0 => None,
            1 => Some(Bag {
                row: input.u8()?,
                at: [input.f32()?, input.f32()?, input.f32()?],
                yaw: input.f32()?,
            }),
            _ => return Err(input.invalid("corpse's bag flag", at)),
        };
        Ok(Corpse { due_ms, bag })
    }
}

/// What `thing`'s death leaves, by its appearance (`module::Body`): its
/// bag's row, how long until it is taken away and how long it fades (ms).
fn decay(thing: &Thing) -> (u8, u32, u32) {
    let body = thing.facts.as_ref().and_then(|facts| facts.body);
    body.map_or((0, DESTROY_MS, FADE_MS), |body| {
        (body.bag, body.destroy_ms, body.fade_ms.unwrap_or(FADE_MS))
    })
}

/// Creature `id` died outside the party at `now`: the engine comes for it
/// after its appearance's delay.
pub(super) fn died(view: &View, state: &mut State, id: ObjectId, now: u64) {
    let Some(thing) = view.thing(id) else {
        return;
    };
    let (_, destroy_ms, _) = decay(thing);
    state.corpses.insert(
        id.0,
        Corpse {
            due_ms: now + u64::from(destroy_ms),
            bag: None,
        },
    );
}

/// The corpses due now (`dead`: the creatures dead now, by id): one still
/// dead, kept by no script and not of the party is taken away (its body
/// fading), its remains to come if it leaves any; remains whose body has
/// faded are made, holding what the creature dropped.
pub(super) fn take_away(
    view: &View,
    library: &crate::script::Library,
    state: &mut State,
    out: &mut Out,
    dead: &[ObjectId],
) {
    let now = view.now_ms;
    let due: Vec<(u32, Corpse)> = state
        .corpses
        .iter()
        .filter(|(_, corpse)| corpse.due_ms <= now)
        .map(|(id, corpse)| (*id, *corpse))
        .collect();
    for (id, corpse) in due {
        state.corpses.remove(&id);
        match corpse.bag {
            None => {
                let Some(thing) = view
                    .thing(ObjectId(id))
                    .filter(|_| dead.contains(&ObjectId(id)))
                else {
                    continue;
                };
                let party = thing.character.is_some_and(|character| character.party);
                if party || state.kept.contains(&id) {
                    continue;
                }
                let (row, _, fade_ms) = decay(thing);
                // Its components and chemicals go with it, never in its
                // remains (TSL's own counts).
                state.hoards.remove(&id);
                // Its body fading out, in the world until the fade ends.
                out.changes.push(Change::Destroy {
                    who: thing.id,
                    ms: fade_ms,
                    going: Some(crate::snapshot::Going::Fades(fade_ms)),
                });
                let corpse = bag_in(view.module, row).corpse;
                let drops = state.items.held_by(id).any(|(_, item)| item.droppable);
                if drops || corpse || state.purses.get(&id).is_some_and(|gold| *gold > 0) {
                    let [x, y, z] = thing.position;
                    let feet = [x, y, z + crate::motion::SOLE_Z * collision::UNIT];
                    let wait = fade_ms.max(BAG_AFTER_MS);
                    state.corpses.insert(
                        id,
                        Corpse {
                            due_ms: now + u64::from(wait),
                            bag: Some(Bag {
                                row,
                                at: feet,
                                yaw: thing.yaw,
                            }),
                        },
                    );
                }
            }
            Some(bag) => leave(view, library, state, out, id, bag),
        }
    }
}

/// The remains `bag` of the dead `id`, made: what it dropped moved into
/// them, its gold one stack of credits, the rest it had gone with it.
fn leave(
    view: &View,
    library: &crate::script::Library,
    state: &mut State,
    out: &mut Out,
    id: u32,
    bag: Bag,
) {
    let template = bag_template(bag.row);
    let Some(mut facts) = bag_facts(view.module, &template) else {
        return;
    };
    let made = ObjectId(view.next_id + out.made.len() as u32);
    for item in state.items.ids_held_by(id) {
        let Some(mut held) = state.items.get_mut(&item) else {
            continue;
        };
        if held.droppable {
            (held.holder, held.slot) = (made.0, None);
        } else {
            drop(held);
            state.items.remove(&item);
        }
    }
    if let Some(gold) = state.purses.remove(&id).filter(|gold| *gold > 0) {
        let stack = u16::try_from(gold).unwrap_or(u16::MAX);
        state.make_item(library.items(), CREDITS, made.0, stack, None);
    }
    let holds: Vec<(String, u16)> = state
        .items
        .held_by(made.0)
        .map(|(_, item)| (item.template.clone(), item.stack))
        .collect();
    (facts.position, facts.yaw, facts.inventory) = (bag.at, bag.yaw, held(&holds));
    out.made.push(Made {
        id: made,
        facts: Arc::new(facts),
        at: bag.at,
        yaw: bag.yaw,
        kept: None,
    });
}

/// Remains closed this step (`closed`: by id) holding nothing go, unless
/// they are a corpse.
pub(super) fn closed_empty(view: &View, state: &State, out: &mut Out, closed: &[ObjectId]) {
    for &id in closed {
        let corpse = view
            .thing(id)
            .and_then(|thing| bag_of(view.module, &thing.facts.as_ref()?.template))
            .map(|bag| bag.corpse);
        let empty = || state.items.held_by(id.0).next().is_none();
        if corpse == Some(false) && empty() {
            out.changes.push(Change::destroy(id));
        }
    }
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
