//! The party carries three medpacs, two frag grenades, a thermal detonator,
//! a droid's repair kit and a trinket casting a spell no item use knows
//! (one host, the dev soldier, whose sheet carries other items of its own;
//! the consumables' spells with impact scripts that do nothing, the
//! trinket's with none). Step by step:
//!
//! - its usable kinds become the party's goods, those for its race: not
//!   the repair kit;
//! - a grenade charged and thrown leaves its stack as it flies;
//! - a medpac used leaves its stack as its use begins, and lands in the
//!   step after (a step is longer than the use);
//! - the last frag thrown: the stack is gone, and its kind;
//! - the inventory's Use Item: a medpac used on the host, its heal begun
//!   on the next step as its button's, the medpac kept until it lands (an
//!   action's use, `Pack::ordered`), then one fewer; a thermal detonator
//!   refused (a grenade is used on a foe), its stack kept; the trinket
//!   refused (no impact script does what it does), kept.

use super::{Checks, Ran, casting, run, spells, stage};
use crate::action::Action;
use crate::combat::Sheet;
use crate::input::buttons;
use crate::inventory::{Catalog, ItemFacts, PARTY};
use crate::notice::Notice;
use crate::probe::check;

const MEDPAC: u16 = 64;
const FRAG: u16 = 87;
const THERMAL: u16 = 89;
const TRINKET: u16 = 50;
const REPAIR: u16 = 84;
/// The single-use cost (`iprp_chargecost.2da` row 1).
const SINGLE: u8 = 1;

/// The items' ids, in the order they are made.
const MEDPACS: usize = 0;
const FRAGS: usize = 1;
const THERMALS: usize = 2;
const TRINKETS: usize = 3;

/// The steps after which a grenade has flown, a medpac is in use, its use
/// has landed, the last frag has flown, Use Item used a medpac, refused a
/// grenade (the medpac's heal begun meanwhile) and refused the trinket (the
/// heal landed meanwhile).
const THROWN: usize = 2;
const HEALING: usize = 4;
const HEALED: usize = 5;
const LAST_THROWN: usize = 7;
const USED: usize = 8;
const REFUSED: usize = 9;
const NOTHING: usize = 10;

/// The scenario, and the items' ids.
pub(super) fn scenario() -> Result<Ran, String> {
    let catalog = Catalog::new([
        casting("medpac", MEDPAC, SINGLE),
        casting("frag", FRAG, SINGLE),
        casting("thermal", THERMAL, SINGLE),
        casting("trinket", TRINKET, SINGLE),
        ItemFacts {
            droids: Some(true),
            ..casting("repair", REPAIR, SINGLE)
        },
    ]);
    let (mut world, host, _) = stage(
        Sheet::dev_soldier(),
        catalog.clone(),
        spells(THERMAL, |row| (FRAG..=95).contains(&row), 0),
        &[MEDPAC, FRAG, THERMAL, REPAIR],
        &[],
    )?;
    let mut state = world.script_state().clone();
    let items = [
        ("medpac", 3),
        ("frag", 2),
        ("thermal", 1),
        ("trinket", 1),
        ("repair", 2),
    ]
    .iter()
    .map(|(template, stack)| {
        state
            .make_item(&catalog, template, PARTY, *stack, None)
            .map(|id| id.0)
            .ok_or(format!("no {template}"))
    })
    .collect::<Result<Vec<u32>, String>>()?;
    world.set_script_state(state);
    let use_item = |item| Action::UseItem {
        who: host,
        item,
        target: host,
    };
    let steps = vec![
        (0, Vec::new()),
        (buttons::GRENADE, Vec::new()),
        (0, Vec::new()),
        (0, Vec::new()),
        (buttons::MEDPAC, Vec::new()),
        (0, Vec::new()),
        (buttons::GRENADE, Vec::new()),
        (0, Vec::new()),
        (0, vec![use_item(items[MEDPACS])]),
        (0, vec![use_item(items[THERMALS])]),
        (0, vec![use_item(items[TRINKETS])]),
    ];
    let mut ran = run(world, host, steps);
    ran.items = items;
    Ok(ran)
}

pub(super) fn cases(ran: &Ran) -> Checks {
    let stack =
        |step: usize, at: usize| ran.held(step, ran.items[at]).map_or(0, |(stack, _)| stack);
    let seen = |step: usize| {
        format!(
            "kinds {:?}, medpacs {}, frags {}, thermal {}",
            ran.kinds(step),
            stack(step, MEDPACS),
            stack(step, FRAGS),
            stack(step, THERMALS)
        )
    };
    let fighter = |step: usize| ran.host(step).and_then(|host| host.fighter);
    let healing = fighter(HEALING).and_then(|fighter| fighter.pack.using);
    let healed = fighter(HEALED).map(|fighter| fighter.pack.using);
    let used_pack = fighter(REFUSED).map(|fighter| (fighter.pack.using, fighter.pack.queued));
    let last_used = fighter(THROWN).map(|fighter| fighter.pack.last.0);
    let use_item = Action::UseItem {
        who: ran.host,
        item: ran.items[MEDPACS],
        target: ran.host,
    };
    vec![
        (
            "usable_items_are_the_goods",
            check(
                ran.kinds(0) == [(MEDPAC, 3), (FRAG, 2), (THERMAL, 1)],
                format!("{}, a droid's repair kits left out", seen(0)),
            ),
        ),
        (
            "thrown_grenade_leaves_stack",
            check(
                last_used == Some(FRAG)
                    && stack(THROWN, FRAGS) == 1
                    && ran.kinds(THROWN) == [(MEDPAC, 3), (FRAG, 1), (THERMAL, 1)],
                format!("last used {last_used:?}, {}", seen(THROWN)),
            ),
        ),
        (
            "used_medpac_leaves_stack",
            check(
                healing.is_some_and(|(row, _)| row == MEDPAC)
                    && stack(HEALING, MEDPACS) == 2
                    && healed.is_some_and(|using| using.is_none())
                    && stack(HEALED, MEDPACS) == 2
                    && ran.kinds(HEALED) == [(MEDPAC, 2), (FRAG, 1), (THERMAL, 1)],
                format!(
                    "using {healing:?}, {}; then {healed:?}, {}",
                    seen(HEALING),
                    seen(HEALED)
                ),
            ),
        ),
        (
            "last_of_a_stack_gone",
            check(
                ran.held(LAST_THROWN, ran.items[FRAGS]).is_none()
                    && ran.kinds(LAST_THROWN) == [(MEDPAC, 2), (THERMAL, 1)],
                seen(LAST_THROWN),
            ),
        ),
        (
            "use_item_on_oneself",
            check(
                stack(USED, MEDPACS) == 2
                    && stack(REFUSED, MEDPACS) == 2
                    && stack(NOTHING, MEDPACS) == 1
                    && ran.kinds(NOTHING) == [(MEDPAC, 1), (THERMAL, 1)]
                    && ran.quiet(USED)
                    && used_pack.is_some_and(|(using, queued)| {
                        using.is_some_and(|(row, _)| row == MEDPAC) && queued.is_none()
                    }),
                format!(
                    "{} (as it lands), then using and queued {used_pack:?}, notices {:?}",
                    seen(NOTHING),
                    ran.notices
                ),
            ),
        ),
        (
            "use_item_grenade_refused",
            check(
                ran.told(REFUSED, &Notice::CannotUse) && stack(REFUSED, THERMALS) == 1,
                format!("{}, notices {:?}", seen(REFUSED), ran.notices),
            ),
        ),
        (
            "use_item_doing_nothing_refused",
            check(
                ran.told(NOTHING, &Notice::CannotUse) && stack(NOTHING, TRINKETS) == 1,
                format!(
                    "trinkets {}, notices {:?}",
                    stack(NOTHING, TRINKETS),
                    ran.notices
                ),
            ),
        ),
        (
            "use_item_round_trip",
            check(
                Action::from_bytes(&use_item.to_bytes()).ok().as_ref() == Some(&use_item),
                "UseItem through its bytes".into(),
            ),
        ),
    ]
}
