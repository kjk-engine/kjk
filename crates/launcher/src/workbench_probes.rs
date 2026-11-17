//! `kjk sim probes --only "tsl workbench"`: TSL's prologue's workbench on
//! the game's own data. In the Ebon Hawk (001EBO) as a new game starts it,
//! the party given blasters, the game's own `k_workbench_tut` (what the
//! garage workbench's tutorial runs) opens the bench: item creation on
//! `tutitemcreate.2da`, upgrading disabled, the player's skills. Blasters
//! are broken down until the components reach a repair kit's price
//! (`G_I_DRDREPEQP001`, the "Repair Kit" the tutorial asks for), which is
//! then made: the components spent, the kit carried. A second check makes
//! sure T3-M4 starts the prologue with nothing in his hands, on his sheet
//! as well as among his items. Skipped without TSL.

use std::sync::Arc;

use sim::action::{Action, Workbench};
use sim::notice::{Notice, Screen};
use sim::probe::{Checks, check};
use vfs::Game;

use crate::dialogue_verbs::{Run, play};
use crate::lifecycle::Installer;

const BLASTER: &str = "w_blaste_03";
const KIT: &str = "g_i_drdrepeqp001";

pub fn probes() -> Checks {
    let discovery = crate::discover();
    if discovery.get(Game::Kotor2).is_none() {
        return Vec::new();
    }
    vec![
        ("tsl: the prologue's workbench breaks down and makes a repair kit", run(&discovery)),
        ("tsl: T3-M4 starts the prologue with empty hands", t3_unarmed(&discovery)),
    ]
}

/// TSL's prologue makes T3-M4 from `p_t3m4`, whose template puts a mining
/// laser in his right hand, and `a_bet3m4` destroys that laser in the same
/// step (Discord, 2026-11-17: the equip page showed a blaster in his hand
/// that was no item and could not be taken off). His sheet must be unarmed,
/// as no item is in his hands.
fn t3_unarmed(discovery: &vfs::Discovery) -> Result<String, String> {
    let anims = Arc::new(sim::motion::AnimTable::stand_in());
    let installer = Installer::new(discovery, Game::Kotor2, anims)?;
    let mut world = play(&installer, "001ebo", &Run { seconds: 1, ..Run::default() })?.world;
    let snapshot = world.snapshot();
    let state = world.script_state();
    let t3 = state.party.switched_body().ok_or("the prologue plays no companion")?;
    let in_hands: Vec<&str> = state
        .items
        .values()
        .filter(|item| item.holder == t3 && matches!(item.slot, Some(4 | 5)))
        .map(|item| item.template.as_str())
        .collect();
    let sheet = snapshot
        .get(sim::SimId(t3))
        .and_then(|entity| entity.sheet)
        .ok_or("T3-M4 has no sheet")?;
    let armed = sheet.weapons.map(|weapon| weapon.is_some());
    check(
        in_hands.is_empty() && armed == [false, false],
        format!("items in his hands {in_hands:?}, sheet armed in each hand {armed:?}"),
    )
}

/// Seat 0 stepped once with `action`, if any: what it was told.
fn step(world: &mut sim::SimWorld, action: Option<Action>) -> Vec<Notice> {
    let mut input = sim::TickInput::default();
    input.insert(sim::Seat(0), sim::Command::default());
    if let Some(action) = action {
        input.act(sim::Seat(0), action);
    }
    let tick = world.tick() + 1;
    let rate = sim::TickRate::default();
    sim::step(world, tick, &input, rate.msec(tick), sim::StepReason::Authority);
    world
        .take_notices()
        .into_iter()
        .map(|(_, notice)| notice)
        .collect()
}

fn run(discovery: &vfs::Discovery) -> Result<String, String> {
    let anims = Arc::new(sim::motion::AnimTable::stand_in());
    let installer = Installer::new(discovery, Game::Kotor2, anims)?;
    let run = Run {
        seconds: 1,
        items: vec![(BLASTER.into(), 12)],
        ..Run::default()
    };
    let mut world = play(&installer, "001ebo", &run)?.world;
    crate::by_hand::queue_scripts(&mut world, &["k_workbench_tut".into()])?;
    let opened = (0..4)
        .flat_map(|_| step(&mut world, None))
        .find_map(|notice| match notice {
            Notice::Show(Screen::Workbench(bench)) => Some(bench),
            _ => None,
        })
        .ok_or("k_workbench_tut opened no workbench")?;
    let bench = Workbench {
        list: opened.list.clone(),
        lab: opened.lab,
        maker: opened.maker,
    };
    let library = world.level().scripts().cloned().ok_or("no scripts")?;
    let shop = library.workshop();
    let row = shop
        .table(&bench.list)
        .and_then(|table| table.iter().position(|recipe| recipe.template == KIT))
        .ok_or(format!("{KIT}: not in {}", bench.list))?;
    let kit = library.items().get(KIT).ok_or(format!("{KIT}: no template"))?;
    let price = i32::try_from(shop.price(kit.into())).unwrap_or(i32::MAX);
    let mut yields = Vec::new();
    while world.script_state().components < price {
        let state = world.script_state();
        let Some(blaster) = state
            .items
            .iter()
            .find(|(_, item)| item.template == BLASTER && item.holder == sim::inventory::PARTY)
            .map(|(id, _)| *id)
        else {
            break;
        };
        let before = state.components;
        let told = step(
            &mut world,
            Some(Action::Breakdown {
                bench: bench.clone(),
                item: blaster,
            }),
        );
        yields.push(world.script_state().components - before);
        if world.script_state().components == before {
            return Err(format!("a breakdown came to nothing: {told:?}"));
        }
    }
    let before = world.script_state().components;
    let told = step(
        &mut world,
        Some(Action::Create {
            bench: bench.clone(),
            row: row as u16,
        }),
    );
    let state = world.script_state();
    let kits: Vec<&str> = state
        .items
        .values()
        .filter(|item| item.template == KIT && item.holder == sim::inventory::PARTY)
        .filter_map(|item| Some(library.items().get(&item.template)?.written_tag()))
        .collect();
    let left = state.components;
    check(
        opened.list == "tutitemcreate"
            && !opened.upgrade
            && !yields.is_empty()
            && left == before - price
            && kits == ["G_I_DRDREPEQP001"],
        format!(
            "bench {opened:?}; blasters broken down for {yields:?} components; the kit's price \
             {price} of {before}, {left} left, kits carried {kits:?}, told {told:?}"
        ),
    )
}
