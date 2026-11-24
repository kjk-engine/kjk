//! An item activated where it is worn (`combat::items::Use::Activated`).
//! The approved test `worn_items_activate` reads the item from a UTI it
//! makes: a datapad on the energy shields' base item, which casts a
//! `spells.2da` row the engine does not name. The row's impact script does
//! nothing here. The host wears one of the items on its right arm, and the
//! party carries a second one. Step by step:
//!
//! - its usable kinds are the worn item's uses, and not the carried one's;
//! - its shield button activates the worn item: the activate clip plays, a
//!   charge of the worn item is spent, and the row is cast on the host as
//!   the use lands, with no energy shield raised in its script's place.

use super::{Checks, Ran, run, spells, stage};
use crate::combat::Sheet;
use crate::combat::items::clips;
use crate::input::buttons;
use crate::inventory::{Catalog, ItemFacts, PARTY, slots};
use crate::probe::check;

/// The worn item and the carried one, in the order they are made.
const WORN: usize = 0;
const CARRIED: usize = 1;

/// The steps after which the button is pressed and the use lands.
const PRESSED: usize = 1;
const LANDED: usize = 2;

/// `item` worn on the host's right arm and carried by the party, and the
/// host's shield button pressed once.
fn scenario(item: &ItemFacts) -> Result<Ran, String> {
    let row = item.spell.ok_or("the item casts no spell")?.row;
    let catalog = Catalog::new([item.clone()]);
    let (mut world, host, _) = stage(
        Sheet::dev_soldier(),
        catalog.clone(),
        spells(row, |_| false, 0),
        &[row],
        &[],
    )?;
    let mut state = world.script_state().clone();
    let items = [(host, Some(slots::RIGHT_ARM)), (PARTY, None)]
        .iter()
        .map(|(holder, slot)| {
            state
                .make_item(&catalog, &item.template, *holder, 1, *slot)
                .map(|id| id.0)
                .ok_or(format!("no {}", item.template))
        })
        .collect::<Result<Vec<u32>, String>>()?;
    world.set_script_state(state);
    let steps = vec![
        (0, Vec::new()),
        (buttons::SHIELD, Vec::new()),
        (0, Vec::new()),
    ];
    let mut ran = run(world, host, steps);
    ran.items = items;
    Ok(ran)
}

fn cases(ran: &Ran, row: u16) -> Checks {
    let charges =
        |step: usize, at: usize| ran.held(step, ran.items[at]).map(|(_, charges)| charges);
    let seen = |step: usize| {
        format!(
            "kinds {:?}, charges: worn {:?}, carried {:?}, notices {:?}",
            ran.kinds(step),
            charges(step, WORN),
            charges(step, CARRIED),
            ran.notices
        )
    };
    let clip = ran
        .host(PRESSED)
        .and_then(|host| host.fighter)
        .and_then(|fighter| fighter.acting)
        .map(|acting| acting.animation);
    let cast_on = ran.states[LANDED]
        .last
        .get(&(ran.host, crate::script::last::SPELL_TARGET))
        .copied();
    let shield = ran
        .host(LANDED)
        .and_then(|host| host.fighter)
        .map(|fighter| fighter.pack.shield.is_some());
    vec![
        (
            "worn_item_is_usable",
            check(ran.kinds(0) == [(row, 10)], seen(0)),
        ),
        (
            "worn_item_activated",
            check(
                clip == Some(clips::ACTIVATE)
                    && charges(PRESSED, WORN) == Some(9)
                    && charges(PRESSED, CARRIED) == Some(10)
                    && ran.kinds(PRESSED) == [(row, 9)]
                    && ran.quiet(PRESSED),
                format!("clip {clip:?}, {}", seen(PRESSED)),
            ),
        ),
        (
            "worn_item_script_cast",
            check(
                cast_on == Some(ran.host) && shield == Some(false),
                format!(
                    "cast on {cast_on:?}, energy shield up {shield:?}, {}",
                    seen(LANDED)
                ),
            ),
        ),
    ]
}

/// The checks of `item` (ten charges, one spent a use) activated where it
/// is worn, and that two runs end alike.
pub fn probes(item: &ItemFacts) -> Checks {
    let Some(row) = item.spell.map(|spell| spell.row) else {
        return vec![("worn_item_is_usable", Err("the item casts no spell".into()))];
    };
    super::twice(
        "worn_item_same_every_run",
        || scenario(item),
        |ran| cases(ran, row),
    )
}
