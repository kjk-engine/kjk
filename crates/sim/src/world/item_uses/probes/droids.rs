//! The host is a droid wearing a stun ray and a flamethrower on its arms
//! and an energy shield on its belt (their spells with impact scripts that
//! do nothing; ten charges each, one a use, the flamethrower down to its
//! last), a creature beside it. Step by step:
//!
//! - its usable kinds are its worn shield's uses;
//! - its stun ray used on the creature: a charge spent;
//! - its shield button uses the worn shield: a charge spent as the use
//!   begins, and the shield's spell cast on itself as it lands (the step
//!   after: a step is longer than the use), its script doing what it does
//!   (here nothing);
//! - the stun ray on itself refused (a weapon is used on a foe);
//! - the flamethrower's last charge: the item is discarded.

use super::{Checks, Ran, casting, run, spells, stage};
use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::sheet::races;
use crate::input::buttons;
use crate::inventory::{Catalog, ItemFacts, slots};
use crate::notice::Notice;
use crate::probe::check;

const SHIELD: u16 = 110;
const STUN_RAY: u16 = 116;
const FLAMER: u16 = 120;
/// One charge a use (`iprp_chargecost.2da` row 6).
const ONE_CHARGE: u8 = 6;

/// The items' ids, in the order they are made.
const STUN_RAYS: usize = 0;
const FLAMERS: usize = 1;
const BELT: usize = 2;

/// The steps after which the stun ray is used, the shield raised, the
/// stun ray refused on itself and the flamethrower spent.
const STUNNED: usize = 1;
const SHIELDED: usize = 2;
const REFUSED: usize = 3;
const SPENT: usize = 4;

/// A droid's item casting `row`, worn in `slots`, with ten charges.
fn droid_item(template: &str, row: u16, slots: u32) -> ItemFacts {
    ItemFacts {
        slots,
        charges: 10,
        droids: Some(true),
        stacking: 1,
        ..casting(template, row, ONE_CHARGE)
    }
}

pub(super) fn scenario() -> Result<Ran, String> {
    let arms = 1 << slots::LEFT_ARM | 1 << slots::RIGHT_ARM;
    let catalog = Catalog::new([
        droid_item("stunray", STUN_RAY, arms),
        droid_item("flamer", FLAMER, arms),
        droid_item("shieldbelt", SHIELD, 1 << slots::BELT),
    ]);
    let droid = Sheet {
        race: races::DROID,
        ..Sheet::dev_soldier()
    };
    let (mut world, host, placed) = stage(
        droid,
        catalog.clone(),
        spells(125, |row| (STUN_RAY..=125).contains(&row), 150),
        &[STUN_RAY, FLAMER, SHIELD],
        &["foe"],
    )?;
    let foe = placed[0];
    let mut state = world.script_state().clone();
    let items = [
        ("stunray", slots::RIGHT_ARM),
        ("flamer", slots::LEFT_ARM),
        ("shieldbelt", slots::BELT),
    ]
    .iter()
    .map(|(template, slot)| {
        state
            .make_item(&catalog, template, host, 1, Some(*slot))
            .map(|id| id.0)
            .ok_or(format!("no {template}"))
    })
    .collect::<Result<Vec<u32>, String>>()?;
    if let Some(mut flamer) = state.items.get_mut(&items[FLAMERS]) {
        flamer.charges = 1;
    }
    world.set_script_state(state);
    let use_on = |item, target| Action::UseItem {
        who: host,
        item,
        target,
    };
    let steps = vec![
        (0, Vec::new()),
        (0, vec![use_on(items[STUN_RAYS], foe)]),
        (buttons::SHIELD, Vec::new()),
        (0, vec![use_on(items[STUN_RAYS], host)]),
        (0, vec![use_on(items[FLAMERS], foe)]),
        (0, Vec::new()),
    ];
    let mut ran = run(world, host, steps);
    ran.items = items;
    Ok(ran)
}

pub(super) fn cases(ran: &Ran) -> Checks {
    let charges =
        |step: usize, at: usize| ran.held(step, ran.items[at]).map(|(_, charges)| charges);
    let seen = |step: usize| {
        format!(
            "kinds {:?}, charges: stun ray {:?}, flamethrower {:?}, shield {:?}, notices {:?}",
            ran.kinds(step),
            charges(step, STUN_RAYS),
            charges(step, FLAMERS),
            charges(step, BELT),
            ran.notices
        )
    };
    // Cast on itself as the use lands, no native shield up in its
    // script's place.
    let cast_on = ran.states[REFUSED]
        .last
        .get(&(ran.host, crate::script::last::SPELL_TARGET))
        .copied();
    let shield = ran
        .host(REFUSED)
        .and_then(|host| host.fighter)
        .map(|fighter| fighter.pack.shield.map(|shield| shield.row));
    vec![
        (
            "worn_shield_is_usable",
            check(ran.kinds(0) == [(SHIELD, 10)], seen(0)),
        ),
        (
            "droid_weapon_on_a_foe",
            check(
                charges(STUNNED, STUN_RAYS) == Some(9) && ran.quiet(STUNNED),
                seen(STUNNED),
            ),
        ),
        (
            "worn_shield_raised",
            check(
                cast_on == Some(ran.host)
                    && shield == Some(None)
                    && charges(SHIELDED, BELT) == Some(9)
                    && ran.kinds(SHIELDED) == [(SHIELD, 9)],
                format!(
                    "cast on {cast_on:?}, native shield {shield:?}, {}",
                    seen(SHIELDED)
                ),
            ),
        ),
        (
            "droid_weapon_not_on_itself",
            check(
                ran.told(REFUSED, &Notice::CannotUse) && charges(REFUSED, STUN_RAYS) == Some(9),
                seen(REFUSED),
            ),
        ),
        (
            "last_charge_discards",
            check(
                charges(SPENT, FLAMERS).is_none() && ran.quiet(SPENT),
                seen(SPENT),
            ),
        ),
    ]
}
