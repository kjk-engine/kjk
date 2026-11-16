//! The probes of armour barring Force powers (issue 428), on the proving
//! ground, no install. Both games' executables refuse a power when a cast
//! begins if the items worn bar it: its `spells.2da` `forbiditemmask` has a
//! bit of an item worn (each armour class from 4 to 9 sets its own), or its
//! `requireitemmask` asks for one not worn (a lightsaber, for the saber
//! throw).
//! - The power key with a barred power selected casts nothing and costs
//!   nothing. Without the armour the same press casts it and pays.
//! - A queued cast (a script's or an AI talent's) of a barred power ends
//!   at once, and the same cast without the armour runs its impact.
//! - A worn item's bit reaches the sheet from any slot, the second weapon
//!   set's too.
//! - A row that requires an item is barred until it is worn.

use super::power_cycle_probes::{
    COST, FORCE_BARRIER, become_, cast, player, step, token, world_with,
};
use crate::combat::Sheet;
use crate::combat::force::items_bar;
use crate::input::Command;
use crate::inventory::{ItemFacts, dress, slots};
use crate::probe::{Checks, check};
use crate::script::actions::{Action, Target};
use script_ncs::vm::ObjectId;

/// Force Barrier's token (`power_cycle_probes::ROWS`).
const BARRIER_TOKEN: i32 = 40;
/// What the probes' Force Barrier forbids: every armour class.
const ANY_ARMOUR: u8 = 0x3f;
/// The bit of a class 6 armour, and of a lightsaber.
const CLASS_6: u8 = 0x04;
const SABER: u8 = 0x40;

pub fn probes() -> Checks {
    vec![
        (
            "the power key casts no power the armour bars",
            key_refused(),
        ),
        (
            "a queued cast of a power the armour bars ends",
            queue_refused(),
        ),
        ("every slot's item reaches the mask", mask_of_slots()),
        ("a required item allows the power", required()),
    ]
}

/// The soldier knowing Force Barrier, wearing `worn`.
fn wearing(worn: u8) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.powers.insert(FORCE_BARRIER);
    sheet.gear.force_items = worn;
    sheet
}

/// The probes' world with Force Barrier forbidden to any armour.
fn barred_world() -> Result<super::SimWorld, String> {
    world_with(
        wearing(0),
        |menu| menu,
        |data| {
            data.force_items = vec![(0, 0); usize::from(FORCE_BARRIER) + 1];
            data.force_items[usize::from(FORCE_BARRIER)] = (ANY_ARMOUR, 0);
        },
    )
}

fn key_refused() -> Result<String, String> {
    let mut world = barred_world()?;
    step(&mut world, Command::default());
    become_(&mut world, wearing(CLASS_6));
    let armoured = cast(&mut world, FORCE_BARRIER, BARRIER_TOKEN);
    become_(&mut world, wearing(0));
    let bare = cast(&mut world, FORCE_BARRIER, BARRIER_TOKEN);
    let paid = |fp: i32| (i32::from(COST - 1) * 100..=i32::from(COST) * 100).contains(&fp);
    check(
        armoured.0 == "-" && armoured.1 <= 0 && bare.0 == "1" && paid(bare.1),
        format!(
            "in armour: impact {:?}, {} hundredths paid; without: impact {:?}, {} paid",
            armoured.0, armoured.1, bare.0, bare.1
        ),
    )
}

/// A cast of Force Barrier queued on the player, then steps: its impact's
/// token.
fn queued(world: &mut super::SimWorld) -> String {
    let Some((id, _)) = player(world) else {
        return "no player".into();
    };
    let who = ObjectId(id.0);
    let mut state = world.script_state().clone();
    state.queue(
        who,
        Action::Cast {
            row: FORCE_BARRIER,
            target: Target::Object(who),
            fake: false,
            cheat: false,
        },
    );
    world.set_script_state(state);
    let mut last = step(world, Command::default());
    for _ in 0..10 {
        last = step(world, Command::default());
    }
    token(&last, BARRIER_TOKEN)
}

fn queue_refused() -> Result<String, String> {
    let mut world = barred_world()?;
    step(&mut world, Command::default());
    become_(&mut world, wearing(CLASS_6));
    let armoured = queued(&mut world);
    become_(&mut world, wearing(0));
    let bare = queued(&mut world);
    check(
        armoured == "-" && bare == "1",
        format!("in armour: impact {armoured:?}; without: {bare:?}"),
    )
}

fn mask_of_slots() -> Result<String, String> {
    let item = |force_item| ItemFacts {
        force_item,
        ..ItemFacts::plain("probe_item")
    };
    let (armour, saber) = (item(CLASS_6), item(SABER));
    let sheet = dress(
        Sheet::dev_soldier(),
        [(slots::BODY, &armour), (slots::RIGHT_WEAPON_2, &saber)],
    );
    check(
        sheet.gear.force_items == CLASS_6 | SABER,
        format!("mask {:#04x}", sheet.gear.force_items),
    )
}

fn required() -> Result<String, String> {
    let throw = (0, SABER);
    let barred = [0, CLASS_6, SABER, SABER | CLASS_6].map(|worn| items_bar(throw, worn));
    let armour = [0, CLASS_6, SABER].map(|worn| items_bar((ANY_ARMOUR, 0), worn));
    check(
        barred == [true, true, false, false] && armour == [false, true, false],
        format!("the throw barred {barred:?}; an armour-barred power {armour:?}"),
    )
}
