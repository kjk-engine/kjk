//! The combat arena (`kjk play --stage arena`): a test floor where the
//! party fights whatever its players call in. A level is an arena when it
//! has `Rules` (`Level::with_arena`); there:
//!
//! - a player's `Action::Arena` orders are carried out on the authority, in
//!   the scripts' step (`orders`): a creature of a template made ahead of
//!   the seat's body and turned hostile, every creature not of the party
//!   taken away, or the body's god mode (`sheet::flags::IMMORTAL`) turned
//!   on or off;
//! - one whose body falls below the pit's floor dies there (a party member
//!   goes down), and a party member down is back on its feet at the
//!   respawn point, whole, after `Rules::respawn_ms`: nobody revives and
//!   the arena is never lost (`falls`);
//! - its props (`Rules::props`, the prop hall's) are laid out whenever none
//!   of them stands: placeables of a `placeables.2da` row each, made from a
//!   template that names the row (`prop_template`), as remains are.

use std::sync::Arc;

use script_ncs::vm::ObjectId;

use crate::action::{Action, ArenaOrder};
use crate::combat::sheet::{factions, flags};
use crate::input::{Seat, TickInput};
use crate::module::{ObjectData, ObjectKind};
use crate::script::{self, Change, Library, Out, State, View};

/// An arena's rules (metres, degrees and sim ms).
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// Where the party comes back, and the way it then faces.
    pub respawn: [f32; 3],
    pub yaw: f32,
    /// The height under which a body has fallen into the pit.
    pub pit: f32,
    /// How long a party member lies down before it comes back.
    pub respawn_ms: u64,
    /// The props it lays out (`lay_out`), in their order.
    pub props: Arc<[Prop]>,
}

/// A prop an arena lays out: a placeable drawn as `placeables.2da` row
/// `appearance`, named `name`, standing at `at` (metres) facing `yaw`
/// (degrees) and colliding as `footprint` (`ObjectData::geometry`: least and
/// greatest corners in pairs, in its own frame). It holds nothing and is not
/// used; blows break it or show on it as `destructible` says, with the
/// catalogue's hit points.
#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub appearance: u16,
    pub name: String,
    pub at: [f32; 3],
    pub yaw: f32,
    pub footprint: Vec<[f32; 3]>,
    pub destructible: Option<crate::destruction::Destructible>,
}

impl Prop {
    /// Its facts, as made.
    fn facts(&self) -> ObjectData {
        let most = self.destructible.map(crate::destruction::most_hp);
        ObjectData {
            kind: ObjectKind::Placeable,
            name: self.name.clone(),
            geometry: self.footprint.clone(),
            destructible: self.destructible,
            hit_points: most.map(|most| (most, most)),
            ..ObjectData::unknown(&prop_template(self.appearance), self.at, self.yaw)
        }
    }
}

/// The template an arena's prop of `placeables.2da` row `appearance` is
/// made from.
pub fn prop_template(appearance: u16) -> String {
    format!("{PROP_PREFIX}{appearance}")
}

/// The `placeables.2da` row of an arena's prop made from `template`, if it
/// is one.
pub fn prop_row(template: &str) -> Option<u16> {
    template.strip_prefix(PROP_PREFIX)?.parse().ok()
}

const PROP_PREFIX: &str = "*prop";

impl Rules {
    pub(super) fn digest(&self, hasher: &mut blake3::Hasher) {
        for value in self.respawn.iter().chain([&self.yaw, &self.pit]) {
            hasher.update(&value.to_le_bytes());
        }
        hasher.update(&self.respawn_ms.to_le_bytes());
        for prop in self.props.iter() {
            hasher.update(&prop.appearance.to_le_bytes());
            hasher.update(prop.name.as_bytes());
            let corners = prop.footprint.iter().flatten();
            for value in prop.at.iter().chain([&prop.yaw]).chain(corners) {
                hasher.update(&value.to_le_bytes());
            }
            if let Some(d) = prop.destructible {
                hasher.update(&[d.class as u8, d.material as u8, d.size as u8]);
            }
        }
    }

    /// The facts of its prop made from `template`, if it has one.
    pub(crate) fn prop_facts(&self, template: &str) -> Option<ObjectData> {
        let row = prop_row(template)?;
        let prop = self.props.iter().find(|prop| prop.appearance == row)?;
        Some(prop.facts())
    }
}

/// How far ahead of the seat's body a creature called in is made (metres).
pub const SPAWN_AHEAD: f32 = rules::world::arena::SPAWN_AHEAD_M;

/// The arena's orders this step (see the module documentation), in the
/// order they came; `acting` is each seat's body.
pub(super) fn orders(
    rules: &Rules,
    library: &Library,
    view: &View,
    state: &mut State,
    inputs: &TickInput,
    acting: impl Fn(Seat) -> Option<ObjectId>,
    out: &mut Out,
) {
    for (seat, action) in inputs.actions() {
        let Action::Arena(order) = action else {
            continue;
        };
        let Some(body) = acting(*seat).and_then(|body| view.thing(body)) else {
            continue;
        };
        match order {
            ArenaOrder::Spawn(template) => {
                let template = script::key(template);
                let Some(mut sheet) = library.creature(&template).and_then(|facts| facts.sheet)
                else {
                    continue;
                };
                let (sin, cos) = crate::math::sin_cos_deg(body.yaw);
                let [x, y, z] = body.position;
                let at = [x + cos * SPAWN_AHEAD, y + sin * SPAWN_AHEAD, z];
                let yaw = body.yaw + 180.0;
                let Some(made) = script::make(library, view, state, out, &template, at, yaw) else {
                    continue;
                };
                // A foe of the party whatever its template's side, killable:
                // its sheet as made (which the snapshot keeps).
                sheet.faction = factions::HOSTILE_1;
                sheet.flags &= !(flags::PLOT | flags::MIN_ONE_HP | flags::IMMORTAL);
                out.changes.push(Change::Sheet { who: made, sheet });
            }
            ArenaOrder::Clear => {
                let foes = view.things.iter().filter(|thing| {
                    thing.kind == Some(ObjectKind::Creature)
                        && thing.character.is_some_and(|character| !character.party)
                });
                for thing in foes {
                    out.changes.push(Change::Destroy {
                        who: thing.id,
                        ms: 0,
                        going: None,
                    });
                }
            }
            ArenaOrder::God => {
                let Some(character) = body.character else {
                    continue;
                };
                let mut sheet = out.sheet(body.id, character.worn);
                sheet.flags ^= flags::IMMORTAL;
                out.changes.push(Change::Sheet {
                    who: body.id,
                    sheet,
                });
            }
            // A fall of the body's own (`falls::arena_falls`).
            ArenaOrder::Down(_) => {}
        }
    }
    lay_out(rules, library, view, state, out);
}

/// `rules`' props made where they stand, when none of them does.
fn lay_out(rules: &Rules, library: &Library, view: &View, state: &mut State, out: &mut Out) {
    let standing = view.things.iter().any(|thing| {
        thing
            .facts
            .as_ref()
            .is_some_and(|facts| prop_row(&facts.template).is_some())
    });
    if standing
        || out
            .made
            .iter()
            .any(|made| prop_row(&made.facts.template).is_some())
    {
        return;
    }
    for prop in rules.props.iter() {
        script::make_facts(
            library,
            view,
            state,
            out,
            Arc::new(prop.facts()),
            prop.at,
            prop.yaw,
        );
    }
}
