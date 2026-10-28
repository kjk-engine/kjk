//! Items used (their Cast Spell property, `inventory::ItemSpell`): each
//! use leaves the goods in the step it is made (`spend`: one of a stack,
//! or a charge), and every character's usable kinds (`Sheet::items`, the
//! HUD's slots and the buttons' items) are the goods (`usable`): what its
//! holder carries (the party's for a party member, a creature's own
//! otherwise) and what it wears (a droid's shield). The goods are the one
//! truth; a fighter's `Pack::used` counts only the uses they do not show
//! yet. A character's kinds are only those for its race (`droidorhuman`,
//! `ItemFacts::fits`), as Use Item and the AI's item talents have them,
//! and only those whose spell has an impact script (`inventory::usable`).

use std::collections::{BTreeMap, BTreeSet};

use script_ncs::vm::ObjectId;

use crate::combat::Sheet;
use crate::combat::items::KINDS;
use crate::inventory::{Catalog, ItemFacts};
use crate::script::{Change, Library, Out, State, View, items::holder};
use crate::snapshot::SimId;

#[cfg(feature = "probes")]
mod probes;

#[cfg(feature = "probes")]
pub use probes::probes;

/// A character as the step found it: who holds its carried items (the
/// party, or itself), its sheet and its uses not yet taken from them.
pub(super) struct Character {
    pub id: ObjectId,
    pub holder: u32,
    pub sheet: Sheet,
    pub used: [u8; KINDS],
}

/// The uses made this step (by user and row) taken from the goods: the
/// first item casting the row that the user's holder carries or the user
/// wears (`State::casting`). Those taken, by user and row.
pub(super) fn spend(
    state: &mut State,
    catalog: &Catalog,
    view: &View,
    out: &mut Out,
    spent: &[(SimId, u16)],
) -> Vec<(u32, u16)> {
    spent
        .iter()
        .filter(|(user, row)| {
            let user = ObjectId(user.0);
            state
                .casting(catalog, holder(view, user), user, *row)
                .is_some_and(|item| state.spend(catalog, out, item))
        })
        .map(|(user, row)| (user.0, *row))
        .collect()
}

/// Each character's usable kinds as its holder's goods have them, where
/// those goods hold anything for it or its sheet shows kinds it no longer
/// holds (a world whose goods are not kept, a test's arena, keeps its
/// sheets' own): a change for each one whose kinds
/// moved or whose uses were `taken` this step, its uses not yet taken
/// moved with their kinds.
pub(super) fn usable(
    state: &State,
    library: &Library,
    characters: &[Character],
    taken: &[(u32, u16)],
    changes: &mut Vec<Change>,
) {
    // What each holder carries or wears that may be used as it is held,
    // with its uses left: only the characters' holders', as the items of
    // every module left stay in play.
    let wanted: BTreeSet<u32> = characters
        .iter()
        .flat_map(|character| [character.holder, character.id.0])
        .collect();
    let mut ready: BTreeMap<u32, Vec<(u16, &ItemFacts)>> = BTreeMap::new();
    let mut held = BTreeSet::new();
    let catalog = library.items();
    for (_, item) in wanted
        .iter()
        .flat_map(|holder| state.items.held_by(*holder))
    {
        held.insert(item.holder);
        if let Some(facts) = state.item_facts(catalog, &item.template)
            && facts.ready(item.slot.is_some())
        {
            ready
                .entry(item.holder)
                .or_default()
                .push((item.uses(facts), facts));
        }
    }
    for character in characters {
        let who = character.id;
        let from = character.holder;
        let holds = held.contains(&from) || held.contains(&who.0);
        let shows = character.sheet.items.iter().any(Option::is_some);
        if state.items.is_empty() || !(holds || shows) {
            continue;
        }
        let holders = if from == who.0 {
            vec![from]
        } else {
            vec![from, who.0]
        };
        let sheet = changes
            .iter()
            .rev()
            .find_map(|change| match change {
                Change::Sheet { who: of, sheet } if *of == who => Some(*sheet),
                _ => None,
            })
            .unwrap_or(character.sheet);
        let items = crate::inventory::usable(
            sheet.race,
            holders
                .iter()
                .filter_map(|holder| ready.get(holder))
                .flatten()
                .map(|(uses, facts)| (*uses, *facts)),
            |row| library.impact(row).is_some(),
        );
        let took = |row: u16| {
            taken
                .iter()
                .filter(|(user, of)| *user == who.0 && *of == row)
                .count()
        };
        if items == sheet.items && taken.iter().all(|(user, _)| *user != who.0) {
            continue;
        }
        let used = std::array::from_fn(|at| {
            let Some(kind) = items[at] else {
                return 0;
            };
            let before = character
                .sheet
                .items
                .iter()
                .position(|was| was.is_some_and(|was| was.row == kind.row))
                .map_or(0, |was| character.used[was]);
            before.saturating_sub(took(kind.row).min(255) as u8)
        });
        changes.push(Change::Usable {
            who,
            sheet: Sheet { items, ..sheet },
            used,
        });
    }
}
