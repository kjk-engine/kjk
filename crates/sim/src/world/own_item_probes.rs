//! `kjk sim probes --only "own items"`: what an item is when several
//! modules give one resref an item of their own (`inventory::own_template`),
//! and what a creature joining the party brings to it
//! (`State::takes_in`); the script state alone, items of our own.
//!
//! - A module's own template is found by its resref and reads as that
//!   module's: its template names the module. Its resref still names it
//!   (`inventory::names`), as a route's `put tar05_synthodor` does.
//! - Two modules' own datapads of one resref (K1's `g_i_datapad002`: Calo
//!   Nord's journal, the Sith base's log) are two items, neither joining
//!   the other, each with its own tag after the first module is left; two
//!   of one module's still make one stack.
//! - A creature joining: what it carries is the party's (Mission's
//!   vibroblade, unmarked `Dropable` on her template, may still come back
//!   out of a container), joining the party's stacks; what it wears stays
//!   on it.

use crate::inventory::{
    Catalog, ItemFacts, PARTY, module_of, names, own_template, resref_of, slots,
};
use crate::probe::{Checks, check};
use crate::script::State;

const DATAPAD: &str = "g_i_datapad002";
const TARIS_SPACEPORT: &str = "tar_m08aa";
const SITH_BASE: &str = "tar_m09aa";

/// The joining creature's id.
const JOINER: u32 = 500;

/// An item of `template`, tagged `tag`, stacking to `stacking`.
fn item(template: &str, tag: &str, stacking: u16) -> ItemFacts {
    ItemFacts {
        tag: tag.into(),
        stacking,
        ..ItemFacts::plain(template)
    }
}

/// Module `module`'s own datapad, tagged `tag`.
fn datapad(module: &str, tag: &str) -> ItemFacts {
    item(&own_template(DATAPAD, module), tag, 99)
}

/// The shared items, and `module`'s own datapad tagged `tag`.
fn catalog(module: &str, tag: &str) -> Catalog {
    let mut vest = item("vest", "vest", 1);
    vest.slots = 1 << slots::BODY;
    Catalog::new([
        item("vibroblade", "vibroblade", 1),
        item("medpac", "medpac", 10),
        vest,
    ])
    .with(Catalog::new([datapad(module, tag)]))
}

/// The tag of item `id`'s template, as `catalog` and what the state knows
/// read it.
fn tag(state: &State, catalog: &Catalog, id: u32) -> String {
    state
        .items
        .get(&id)
        .and_then(|item| state.item_facts(catalog, &item.template))
        .map_or_else(String::new, |facts| facts.tag.clone())
}

pub fn probes() -> Checks {
    let mut checks = Checks::new();
    let spaceport = catalog(TARIS_SPACEPORT, "tar_calojournal");
    let base = catalog(SITH_BASE, "tar09_techlog");

    let found = spaceport.get(DATAPAD).map(|facts| facts.template.clone());
    let own = found.as_deref().unwrap_or("");
    checks.push((
        "a module's own template: found by its resref, named for its module, named by its resref",
        check(
            resref_of(own) == DATAPAD
                && module_of(own) == Some(TARIS_SPACEPORT)
                && names(own, DATAPAD)
                && names(own, &DATAPAD.to_ascii_uppercase())
                && names(own, own)
                && !names(own, "vibroblade")
                && spaceport.get("vibroblade").is_some()
                && resref_of("vibroblade") == "vibroblade"
                && module_of("vibroblade").is_none(),
            format!("{found:?}"),
        ),
    ));

    let mut state = State::default();
    let journal = state.make_item(&spaceport, DATAPAD, PARTY, 1, None);
    state.carry_known(&spaceport, &base);
    let log = state.make_item(&base, DATAPAD, PARTY, 1, None);
    let (journal, log) = (journal.map_or(0, |id| id.0), log.map_or(0, |id| id.0));
    let stacks: Vec<u16> = [journal, log]
        .iter()
        .filter_map(|id| Some(state.items.get(id)?.stack))
        .collect();
    let tags = [tag(&state, &base, journal), tag(&state, &base, log)];
    checks.push((
        "two modules' own datapads of one resref: two items, each its own tag",
        check(
            journal != log && stacks == [1, 1] && tags == ["tar_calojournal", "tar09_techlog"],
            format!("items {journal} and {log}, stacks {stacks:?}, tags {tags:?}"),
        ),
    ));

    let again = state.make_item(&base, DATAPAD, PARTY, 1, None);
    let stack = state.items.get(&log).map_or(0, |item| item.stack);
    checks.push((
        "two of one module's own: one stack",
        check(
            again.map(|id| id.0) == Some(log) && stack == 2,
            format!("joined {again:?}, its stack {stack}"),
        ),
    ));

    let mut state = State::default();
    let party_medpacs = state.make_item(&base, "medpac", PARTY, 3, None);
    let blade = state.make_item(&base, "vibroblade", JOINER, 1, None);
    let medpacs = state.make_item(&base, "medpac", JOINER, 2, None);
    let vest = state.make_item(&base, "vest", JOINER, 1, Some(slots::BODY));
    // As Mission's template carries it: no `Dropable` mark.
    if let Some(mut held) = blade.and_then(|id| state.items.get_mut(&id.0)) {
        held.droppable = false;
    }
    state.takes_in(&base, JOINER);
    let holder = |id: Option<script_ncs::vm::ObjectId>| {
        id.and_then(|id| state.items.get(&id.0))
            .map(|item| (item.holder, item.slot, item.stack, item.droppable))
    };
    let (blade, vest, party_medpacs) = (holder(blade), holder(vest), holder(party_medpacs));
    let left = state.carried(JOINER).len();
    checks.push((
        "a creature joining: what it carries the party's (out of a container again), joining its stacks; what it wears its own",
        check(
            blade == Some((PARTY, None, 1, true))
                && party_medpacs == Some((PARTY, None, 5, true))
                && holder(medpacs).is_none()
                && vest == Some((JOINER, Some(slots::BODY), 1, true))
                && left == 0,
            format!(
                "its blade {blade:?}, the party's medpacs {party_medpacs:?}, its vest {vest:?}, {left} left with it"
            ),
        ),
    ));
    checks
}
