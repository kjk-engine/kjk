//! `kjk script equip <game> [module] [--container TAG]`: taking and
//! equipping checked end to end (I19; the Endar Spire's bunk room by
//! default, whose `end_locker01` holds the clothes and the sword Trask asks
//! the player to put on, `k_pend_traskdlg2`). The module is installed with
//! its item templates and one script made here (`probe`):
//!
//! 1. The player, stood by the container, opens it and takes everything:
//!    the items are the party's (credits its gold), gone from the
//!    container.
//! 2. A body item and a weapon are worn by the player's actions; one in a
//!    slot it does not fit is refused. The sheet in the snapshot carries
//!    the weapon and the armour's defence; the goods show them worn.
//! 3. `GetItemInSlot` on `GetFirstPC` (what Trask's gate asks) finds them.
//! 4. Taken off again (`Unequip`), the weapon is the party's; a save
//!    carries the items.

use std::process::ExitCode;

use serde_json::json;
use sim::action::Action;
use sim::inventory::{PARTY, slots};
use sim::module::ObjectKind;
use sim::notice::Notice;
use vfs::Discovery;

use crate::probe::{Call, Probe, int, program, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// How far east of the container the player stands (metres).
const BESIDE: f32 = 1.5;

/// `SetGlobalNumber(name, GetIsObjectValid(GetItemInSlot(slot,
/// GetFirstPC())))`.
fn slot_global(name: &str, slot: u8) -> Vec<Call> {
    vec![
        Call::Routine("GetFirstPC", 0),
        int(i32::from(slot)),
        Call::Routine("GetItemInSlot", 2),
        Call::Routine("GetIsObjectValid", 1),
        text(name),
        Call::Routine("SetGlobalNumber", 2),
    ]
}

pub fn equip(discovery: &Discovery, game: &str, module: &str, container: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let mut templates = None;
        let mut probe = Probe::new(discovery, game, module, |level| {
            let items = level
                .scripts()
                .ok_or("the module has no scripts")?
                .items()
                .clone();
            let gate = [
                slot_global("KJK_BODY", slots::BODY),
                slot_global("KJK_RIGHT", slots::RIGHT_WEAPON),
            ];
            let scripts = [("kjk_gate".to_string(), program(&routines, &gate)?)].into();
            templates = Some(items.clone());
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_items(items),
            )
        })?;
        let templates = templates.ok_or("no templates")?;
        let level = probe.world.level().clone();
        let data = level.module().ok_or("no module")?;
        let (index, locker) = data
            .objects()
            .enumerate()
            .find(|(_, object)| {
                object.kind == ObjectKind::Placeable && object.tag.eq_ignore_ascii_case(container)
            })
            .ok_or(format!("no {container} in {module}"))?;
        let index = index as u32;

        // 1. Taking.
        probe.place(locker.position[0] + BESIDE, locker.position[1])?;
        probe.act(Action::Open(index));
        let (after, told) = probe.act(Action::TakeAll(index));
        let party: Vec<(u32, sim::inventory::Item)> = after
            .goods
            .carried(PARTY)
            .map(|(id, item)| (id, item.clone()))
            .collect();
        let units = party
            .iter()
            .map(|(_, item)| u32::from(item.stack))
            .sum::<u32>()
            + after.goods.gold.max(0) as u32;
        let held: u32 = locker
            .inventory
            .iter()
            .map(|held| u32::from(held.stack))
            .sum();
        let locker_id = after
            .entities
            .iter()
            .find(|entity| entity.kind == sim::Kind::Object(ObjectKind::Placeable, index))
            .map(|entity| entity.id.0)
            .ok_or("the container")?;
        let left = probe.world.script_state().carried(locker_id).len();
        report.check(
            told.is_empty() && units == held && left == 0,
            format!(
                "{container}'s {held} items taken: the party's {:?} and {} credits, {left} left in it",
                party
                    .iter()
                    .map(|(_, item)| format!("{} x{}", item.template, item.stack))
                    .collect::<Vec<_>>(),
                after.goods.gold
            ),
        );

        // 2. Equipping.
        let fits = |slot: u8| {
            party
                .iter()
                .find(|(_, item)| {
                    templates
                        .get(&item.template)
                        .is_some_and(|facts| facts.slots & 1 << slot != 0)
                })
                .cloned()
        };
        let (suit, suit_item) = fits(slots::BODY).ok_or("nothing to wear on the body")?;
        let (blade, blade_item) = fits(slots::RIGHT_WEAPON).ok_or("no weapon taken")?;
        let (_, refused) = probe.act(Action::Equip {
            item: suit,
            slot: slots::RIGHT_WEAPON,
        });
        report.check(
            refused.contains(&Notice::CannotEquip),
            format!("{} in the right hand is refused", suit_item.template),
        );
        let before = after
            .get(probe.me)
            .and_then(|me| me.sheet)
            .ok_or("no sheet")?;
        probe.act(Action::Equip {
            item: suit,
            slot: slots::BODY,
        });
        let (after, told) = probe.act(Action::Equip {
            item: blade,
            slot: slots::RIGHT_WEAPON,
        });
        let sheet = after
            .get(probe.me)
            .and_then(|me| me.sheet)
            .ok_or("no sheet")?;
        let facts = |template: &str| templates.get(template).ok_or(format!("{template}?"));
        let armour = facts(&suit_item.template)?.armour.map_or(0, |a| a.defence);
        let worn: Vec<(Option<u8>, String)> = after
            .goods
            .items
            .iter()
            .filter(|(_, item)| item.holder == probe.me.0)
            .map(|(_, item)| (item.slot, item.template.clone()))
            .collect();
        report.check(
            told.is_empty()
                && sheet.weapons[0] == facts(&blade_item.template)?.weapon
                && sheet.defence == before.defence + armour
                && worn.len() == 2,
            format!(
                "{} worn and {} in the right hand: the sheet's weapon base item {:?}, defence {} → {}; worn {worn:?}",
                suit_item.template,
                blade_item.template,
                sheet.weapons[0].map(|w| w.base_item),
                before.defence,
                sheet.defence
            ),
        );

        // 3. What Trask's gate asks.
        probe.queue("kjk_gate", probe.me);
        probe.step(2);
        let state = probe.world.script_state();
        let asked = (
            sim::script::global(&state.numbers, "KJK_BODY").copied(),
            sim::script::global(&state.numbers, "KJK_RIGHT").copied(),
        );
        report.check(
            asked == (Some(1), Some(1)),
            format!("GetItemInSlot(body, right hand) on GetFirstPC: valid {asked:?}"),
        );

        // 4. Taking off, and a save.
        let (after, _) = probe.act(Action::Unequip(slots::RIGHT_WEAPON));
        let back = after.goods.carried(PARTY).any(|(id, _)| id == blade);
        report.check(
            back && probe
                .world
                .script_state()
                .in_slot(probe.me.0, slots::RIGHT_WEAPON)
                .is_none(),
            format!("{} taken off: the party's again", blade_item.template),
        );
        let snapshot = probe.world.snapshot();
        let state = probe.world.script_state().clone();
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: probe.world.level().id(),
            snapshot: snapshot.clone(),
            script: state.clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        report.check(
            loaded.script.items == state.items && loaded.snapshot.goods == snapshot.goods,
            format!(
                "a save carries the {} items and the goods",
                state.items.len()
            ),
        );
        let faults = probe.world.script_report().faults.clone();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        Ok(json!({ "module": module, "container": container }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
