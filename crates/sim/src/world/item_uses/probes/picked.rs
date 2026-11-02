//! The party carries more kinds than a sheet has slots (one host, the dev
//! soldier; the kinds' spells with impact scripts that do nothing), and picks
//! the kind each key uses (`combat::picks`). Step by step:
//!
//! - the kinds kept leave a place for each key: the medpac, last of them, has
//!   one though four grenades come first;
//! - a grenade picked is the one thrown;
//! - a grenade picked with the button held drops the charge: letting go throws
//!   nothing, and the next press charges and throws the kind picked;
//! - a kind no slot held is picked, gets one, and its key uses it.

use super::{Checks, Ran, casting, run, spells, stage};
use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::spells::{ITEM_ABILITY_ADVANCED_MED_PACK, ITEM_ABILITY_MED_PACK};
use crate::input::buttons;
use crate::inventory::{Catalog, PARTY};
use crate::probe::check;

const GRENADES: [u16; 4] = [87, 88, 89, 90];
const FRAG: u16 = GRENADES[0];
const THERMAL: u16 = GRENADES[2];
const MEDPAC: u16 = ITEM_ABILITY_MED_PACK;
const ADVANCED: u16 = ITEM_ABILITY_ADVANCED_MED_PACK;
/// The single-use cost (`iprp_chargecost.2da` row 1).
const SINGLE: u8 = 1;
const STACK: u16 = 4;

/// The items' ids, in the order they are made.
const FRAGS: usize = 0;
const THERMALS: usize = 2;
const MEDPACS: usize = 4;
const ADVANCEDS: usize = 5;

/// The steps after which: the kinds are the goods; a thermal picked has flown;
/// a charge has been dropped; its let-go has come; a frag picked has flown; an
/// advanced medpac is in use.
const SETTLED: usize = 0;
const THROWN: usize = 3;
const DROPPED: usize = 5;
const LET_GO: usize = 6;
const THROWN_AGAIN: usize = 8;
const USED: usize = 10;

pub(super) fn scenario() -> Result<Ran, String> {
    let made = [
        ("frag", FRAG),
        ("sonic", GRENADES[1]),
        ("thermal", THERMAL),
        ("poison", GRENADES[3]),
        ("medpac", MEDPAC),
        ("advanced", ADVANCED),
    ];
    let catalog = Catalog::new(
        made.iter()
            .map(|(template, row)| casting(template, *row, SINGLE)),
    );
    let (mut world, host, _) = stage(
        Sheet::dev_soldier(),
        catalog.clone(),
        spells(GRENADES[3], |row| GRENADES.contains(&row), 0),
        &made.map(|(_, row)| row),
        &[],
    )?;
    let mut state = world.script_state().clone();
    let items = made
        .iter()
        .map(|(template, _)| {
            state
                .make_item(&catalog, template, PARTY, STACK, None)
                .map(|id| id.0)
                .ok_or(format!("no {template}"))
        })
        .collect::<Result<Vec<u32>, String>>()?;
    world.set_script_state(state);
    let pick = |row| vec![Action::PickItem { row }];
    let steps = vec![
        (0, Vec::new()),
        (0, pick(THERMAL)),
        (buttons::GRENADE, Vec::new()),
        (0, Vec::new()),
        (buttons::GRENADE, Vec::new()),
        (buttons::GRENADE, pick(FRAG)),
        (0, Vec::new()),
        (buttons::GRENADE, Vec::new()),
        (0, Vec::new()),
        (0, pick(ADVANCED)),
        (buttons::MEDPAC, Vec::new()),
    ];
    let mut ran = run(world, host, steps);
    ran.items = items;
    Ok(ran)
}

pub(super) fn cases(ran: &Ran) -> Checks {
    let stack =
        |step: usize, at: usize| ran.held(step, ran.items[at]).map_or(0, |(stack, _)| stack);
    let pack = |step: usize| ran.host(step).and_then(|host| host.fighter).map(|f| f.pack);
    let rows = |step: usize| {
        ran.kinds(step)
            .iter()
            .map(|(row, _)| *row)
            .collect::<Vec<_>>()
    };
    let dropped = pack(DROPPED);
    vec![
        (
            "picked_every_key_has_a_slot",
            check(
                rows(SETTLED).contains(&MEDPAC) && rows(SETTLED).contains(&FRAG),
                format!("{:?}", rows(SETTLED)),
            ),
        ),
        (
            "picked_grenade_is_thrown",
            check(
                stack(THROWN, THERMALS) == STACK - 1 && stack(THROWN, FRAGS) == STACK,
                format!(
                    "thermals {}, frags {}",
                    stack(THROWN, THERMALS),
                    stack(THROWN, FRAGS)
                ),
            ),
        ),
        (
            "picked_with_the_button_held_drops_the_charge",
            check(
                dropped.is_some_and(|pack| pack.charge_off && pack.charging_from.is_none())
                    && stack(LET_GO, FRAGS) == STACK
                    && stack(LET_GO, THERMALS) == STACK - 1
                    && pack(LET_GO).is_some_and(|pack| !pack.charge_off),
                format!(
                    "held {dropped:?}, then frags {}, thermals {}",
                    stack(LET_GO, FRAGS),
                    stack(LET_GO, THERMALS)
                ),
            ),
        ),
        (
            "picked_next_press_throws_it",
            check(
                stack(THROWN_AGAIN, FRAGS) == STACK - 1
                    && stack(THROWN_AGAIN, THERMALS) == STACK - 1,
                format!(
                    "frags {}, thermals {}",
                    stack(THROWN_AGAIN, FRAGS),
                    stack(THROWN_AGAIN, THERMALS)
                ),
            ),
        ),
        (
            "picked_a_kind_without_a_slot_gets_one",
            check(
                !rows(SETTLED).contains(&ADVANCED)
                    && rows(USED).contains(&ADVANCED)
                    && stack(USED, ADVANCEDS) == STACK - 1
                    && stack(USED, MEDPACS) == STACK,
                format!(
                    "kinds {:?} then {:?}, advanced {}, medpacs {}",
                    rows(SETTLED),
                    rows(USED),
                    stack(USED, ADVANCEDS),
                    stack(USED, MEDPACS)
                ),
            ),
        ),
    ]
}
