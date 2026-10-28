//! The host wields a blue lightsaber that takes three crystals (two at
//! once); the party carries them and a red colour crystal. Step by step:
//!
//! - a crystal fitted: it leaves the party, the saber's attack rises;
//! - a second: its damage rises too;
//! - a third refused while both slots are full;
//! - the third in place of the first: the first back to the party;
//! - the second taken out: back to the party, its damage gone;
//! - the red colour crystal fitted: the saber is the red one, its crystals
//!   kept, the blue colour crystal back to the party;
//! - a crystal not fitted cannot be taken out.

use super::{Checks, Ran, run, spells, stage};
use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::gear::Roll;
use crate::combat::sheet::{Weapon, damage_types, feats};
use crate::inventory::properties::OWN_TYPE;
use crate::inventory::{Catalog, ItemFacts, PARTY, Property, Upgrade, slots};
use crate::notice::Notice;
use crate::probe::check;

/// The lightsaber's `baseitems.2da` row.
const SABER: u16 = 8;

/// The crystals: their `upgrade.2da` rows, templates and what each adds.
const CRYSTALS: [(u8, &str, Property); 3] = [
    (0, "rubat", Property::Attack(1)),
    (
        1,
        "damind",
        Property::Damage {
            row: OWN_TYPE,
            roll: Roll {
                dice: 0,
                die: 0,
                bonus: 2,
            },
        },
    ),
    (2, "eralam", Property::Attack(3)),
];

/// A lightsaber of one colour, taking the three crystals.
fn saber(template: &str) -> ItemFacts {
    let weapon = Some(Weapon::plain(
        SABER,
        2,
        8,
        (2, 2),
        damage_types::ENERGY,
        [feats::PROFICIENCY_LIGHTSABER; 3],
    ));
    ItemFacts {
        base_item: SABER,
        slots: 1 << slots::RIGHT_WEAPON,
        weapon,
        bare: weapon,
        upgrades: CRYSTALS
            .iter()
            .map(|(row, template, property)| Upgrade {
                row: *row,
                kind: 0,
                template: template.to_string(),
                properties: vec![property.clone()],
                rows: 0,
            })
            .collect(),
        ..super::super::super::containers::plain(template)
    }
}

/// A colour crystal making the long lightsaber `long`.
fn colour(template: &str, long: &str) -> ItemFacts {
    ItemFacts {
        recolours: vec![long.to_string()],
        ..super::super::super::containers::plain(template)
    }
}

/// The items made, in this order: the saber, the three crystals.
const SABERS: usize = 0;
const RUBAT: usize = 1;
const DAMIND: usize = 2;
const ERALAM: usize = 3;
const RED: usize = 4;

/// The steps after which each was done.
const FIRST: usize = 0;
const SECOND: usize = 1;
const FULL: usize = 2;
const REPLACED: usize = 3;
const TAKEN_OUT: usize = 4;
const RECOLOURED: usize = 5;
const NOT_THERE: usize = 6;

pub(super) fn scenario() -> Result<Ran, String> {
    let mut items = vec![
        saber("saber_blue"),
        saber("saber_red"),
        colour("crystal_blue", "saber_blue"),
        colour("crystal_red", "saber_red"),
    ];
    items.extend(CRYSTALS.iter().map(|(row, template, _)| ItemFacts {
        upgrade: Some((u16::from(*row), 0)),
        ..super::super::super::containers::plain(template)
    }));
    let catalog = Catalog::new(items);
    let (mut world, host, _) = stage(
        Sheet::dev_soldier(),
        catalog.clone(),
        spells(0, |_| false, 0),
        &[],
        &[],
    )?;
    let mut state = world.script_state().clone();
    let made = [
        ("saber_blue", host, Some(slots::RIGHT_WEAPON)),
        ("rubat", PARTY, None),
        ("damind", PARTY, None),
        ("eralam", PARTY, None),
        ("crystal_red", PARTY, None),
    ]
    .iter()
    .map(|(template, holder, slot)| {
        state
            .make_item(&catalog, template, *holder, 1, *slot)
            .map(|id| id.0)
            .ok_or(format!("no {template}"))
    })
    .collect::<Result<Vec<u32>, String>>()?;
    world.set_script_state(state);
    let saber_id = made[SABERS];
    let fit = |at: usize, replace: Option<u8>| Action::Fit {
        item: saber_id,
        upgrade: made[at],
        replace,
    };
    let steps = [
        fit(RUBAT, None),
        fit(DAMIND, None),
        fit(ERALAM, None),
        fit(ERALAM, Some(0)),
        Action::Unfit {
            item: saber_id,
            row: 1,
        },
        fit(RED, None),
        Action::Unfit {
            item: saber_id,
            row: 1,
        },
    ]
    .into_iter()
    .map(|action| (0, vec![action]))
    .collect();
    let mut ran = run(world, host, steps);
    ran.items = made;
    Ok(ran)
}

pub(super) fn cases(ran: &Ran) -> Checks {
    let saber = ran.items[SABERS];
    // The saber's template and upgrades, the host's weapon's to hit and
    // damage bonuses, and the party's carried templates, after a step.
    let seen = |step: usize| {
        let state = &ran.states[step];
        let held = state.items.get(&saber);
        let weapon = ran
            .host(step)
            .and_then(|host| host.sheet)
            .and_then(|sheet| sheet.weapons[0]);
        let mut party: Vec<String> = state
            .carried(PARTY)
            .iter()
            .filter_map(|id| Some(state.items.get(&id.0)?.template.clone()))
            .collect();
        party.sort();
        (
            held.map(|item| (item.template.clone(), item.upgrades)),
            weapon.map(|weapon| (weapon.attack_bonus, weapon.damage_bonus)),
            party,
        )
    };
    let shown = |step: usize| format!("{:?}, notices {:?}", seen(step), ran.notices);
    let saber_is = |step: usize, template: &str, upgrades: u32, bonuses: (i8, i8)| {
        let (held, weapon, _) = seen(step);
        held == Some((template.to_string(), upgrades)) && weapon == Some(bonuses)
    };
    let carries = |step: usize, templates: &[&str]| seen(step).2 == templates;
    vec![
        (
            "crystal_fitted",
            check(
                saber_is(FIRST, "saber_blue", 0b1, (1, 0))
                    && carries(FIRST, &["crystal_red", "damind", "eralam"])
                    && ran.quiet(FIRST),
                shown(FIRST),
            ),
        ),
        (
            "second_crystal_fitted",
            check(saber_is(SECOND, "saber_blue", 0b11, (1, 2)), shown(SECOND)),
        ),
        (
            "full_slots_refuse",
            check(
                ran.told(FULL, &Notice::CannotUpgrade)
                    && saber_is(FULL, "saber_blue", 0b11, (1, 2)),
                shown(FULL),
            ),
        ),
        (
            "crystal_replaced",
            check(
                saber_is(REPLACED, "saber_blue", 0b110, (3, 2))
                    && carries(REPLACED, &["crystal_red", "rubat"]),
                shown(REPLACED),
            ),
        ),
        (
            "crystal_taken_out",
            check(
                saber_is(TAKEN_OUT, "saber_blue", 0b100, (3, 0))
                    && carries(TAKEN_OUT, &["crystal_red", "damind", "rubat"]),
                shown(TAKEN_OUT),
            ),
        ),
        (
            "colour_crystal_recolours",
            check(
                saber_is(RECOLOURED, "saber_red", 0b100, (3, 0))
                    && carries(RECOLOURED, &["crystal_blue", "damind", "rubat"]),
                shown(RECOLOURED),
            ),
        ),
        (
            "not_fitted_not_taken_out",
            check(
                ran.told(NOT_THERE, &Notice::CannotUpgrade)
                    && saber_is(NOT_THERE, "saber_red", 0b100, (3, 0)),
                shown(NOT_THERE),
            ),
        ),
        (
            "fit_round_trip",
            check(
                [
                    Action::Fit {
                        item: saber,
                        upgrade: 7,
                        replace: Some(2),
                    },
                    Action::Unfit {
                        item: saber,
                        row: 1,
                    },
                ]
                .iter()
                .all(|action| Action::from_bytes(&action.to_bytes()).ok().as_ref() == Some(action)),
                "Fit and Unfit through their bytes".into(),
            ),
        ),
    ]
}
