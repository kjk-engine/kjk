//! `kjk script items <game> [module]`: items checked end to end (I19,
//! `sim::inventory`, `sim::script::items`). The module (the Endar Spire by
//! default) is installed with its own item templates and three scripts
//! made here (`probe`):
//!
//! 1. On the first visit every object holds its starting items, and each
//!    creature's sheet carries the weapon its stocked right-hand item is.
//! 2. The player makes a stackable usable item twice (one stack of two in
//!    the party's inventory) and is given 150 gold, 50 taken back.
//! 3. A creature makes a melee weapon and a suit of armour and equips them:
//!    its sheet rides in the snapshot with the new weapon and defence, and
//!    the weapon it held is in its inventory.
//! 4. The player destroys the stack; a save carries the items and gold.

use std::process::ExitCode;

use serde_json::json;
use sim::inventory::{ItemFacts, PARTY, slots};
use vfs::Discovery;

use crate::probe::{Call, Probe, float, int, me, program, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// `CreateItemOnObject(template, OBJECT_SELF, 1)` equipped in `slot` at
/// once (`ActionEquipItem`'s arguments pushed first, the item last).
fn make_and_equip(template: &str, slot: u8) -> Vec<Call> {
    vec![
        int(1),
        int(i32::from(slot)),
        int(1),
        me(),
        text(template),
        Call::Routine("CreateItemOnObject", 3),
        Call::Routine("ActionEquipItem", 3),
    ]
}

/// The first template `pick` takes.
fn first<'a>(
    catalog: &'a sim::inventory::Catalog,
    what: &str,
    pick: impl Fn(&ItemFacts) -> bool,
) -> Result<&'a ItemFacts, String> {
    catalog
        .iter()
        .find(|facts| pick(facts))
        .ok_or(format!("no {what} among the item templates"))
}

pub fn items(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let mut chosen = None;
        let mut probe = Probe::new(discovery, game, module, |level| {
            let own = level.scripts().ok_or("the module has no scripts")?;
            let data = level.module().ok_or("no module")?;
            let templates = own.items().clone();
            let medpac = first(&templates, "stackable usable item", |f| {
                f.combat_row().is_some() && f.stacking > 1
            })?
            .clone();
            // The first creature whose right hand holds a known weapon.
            let (index, held) = data
                .objects()
                .enumerate()
                .filter(|(_, object)| object.kind == sim::module::ObjectKind::Creature)
                .find_map(|(index, object)| {
                    let held = object.inventory.iter().find(|held| {
                        held.slot == Some(slots::RIGHT_WEAPON)
                            && templates
                                .get(&held.template)
                                .is_some_and(|f| f.weapon.is_some())
                    })?;
                    Some((index, templates.get(&held.template)?.clone()))
                })
                .ok_or("no armed creature")?;
            let blade = first(&templates, "melee weapon", |f| {
                f.weapon.is_some_and(|w| {
                    w.reach_m == 0 && Some(w.base_item) != held.weapon.map(|w| w.base_item)
                }) && f.slots & 1 << slots::RIGHT_WEAPON != 0
            })?
            .clone();
            let suit = first(&templates, "armour", |f| {
                f.armour.is_some_and(|a| a.defence > 0) && f.slots & 1 << slots::BODY != 0
            })?
            .clone();
            let make = [
                int(1),
                me(),
                text(&medpac.template),
                Call::Routine("CreateItemOnObject", 3),
            ];
            let gold = vec![
                int(150),
                me(),
                Call::Routine("GiveGoldToCreature", 2),
                int(0),
                me(),
                int(50),
                Call::Routine("TakeGoldFromCreature", 3),
            ];
            let destroy = vec![
                int(0),
                float(0.0),
                int(0),
                float(0.0),
                text(&medpac.tag),
                me(),
                Call::Routine("GetItemPossessedBy", 2),
                Call::Routine("DestroyObject", 5),
            ];
            let mut scripts = std::collections::BTreeMap::new();
            for (name, calls) in [
                ("kjk_items1", vec![make.to_vec(), make.to_vec(), gold]),
                (
                    "kjk_items2",
                    vec![
                        make_and_equip(&blade.template, slots::RIGHT_WEAPON),
                        make_and_equip(&suit.template, slots::BODY),
                    ],
                ),
                ("kjk_items3", vec![destroy]),
            ] {
                scripts.insert(name.to_string(), program(&routines, &calls)?);
            }
            chosen = Some((templates.clone(), medpac, blade, suit, index));
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_items(templates),
            )
        })?;
        let (templates, medpac, blade, suit, index) = chosen.ok_or("nothing chosen")?;
        let state = probe.world.script_state().clone();
        let level = probe.world.level().clone();

        // 1. The first visit's stock.
        let snapshot = probe.world.snapshot();
        let objects = sim::Instances::in_snapshot(&level, &snapshot);
        let ids: Vec<(u32, &sim::module::ObjectData)> = snapshot
            .entities
            .iter()
            .filter_map(|e| Some((e.id.0, objects.of_entity(e)?)))
            .collect();
        let units = |holder: u32| -> u32 {
            state
                .items
                .values()
                .filter(|item| item.holder == holder)
                .map(|item| u32::from(item.stack))
                .sum()
        };
        let stocked = ids.iter().all(|&(id, object)| {
            let held: u32 = object
                .inventory
                .iter()
                .filter(|held| templates.get(&held.template).is_some())
                .map(|held| u32::from(held.stack))
                .sum();
            units(id) == held
        });
        let holders = ids.iter().filter(|&&(id, _)| units(id) > 0).count();
        // What each object starts with, for the facts.
        let stock: Vec<serde_json::Value> = ids
            .iter()
            .filter(|&&(id, _)| units(id) > 0)
            .map(|&(id, object)| {
                let items: Vec<serde_json::Value> = state
                    .items
                    .values()
                    .filter(|item| item.holder == id)
                    .map(|item| json!({ "template": item.template, "slot": item.slot, "stack": item.stack }))
                    .collect();
                json!({ "tag": object.tag, "kind": object.kind.name(), "items": items })
            })
            .collect();
        report.check(
            stocked && !state.items.is_empty(),
            format!(
                "the first visit stocks every object: {} items on {holders} objects, as their templates list",
                state.items.len()
            ),
        );
        // K1's Eriadu stealth unit and minor frag mine (TSL names its own).
        if game == vfs::Game::Kotor1 {
            let unit = templates.get("g_i_belt008");
            let kit = templates.get("g_i_trapkit004");
            report.check(
                unit.is_some_and(|f| f.gear.skills[2] == 6 && f.stealth_unit)
                    && kit.is_some_and(|f| f.trap == Some(3)),
                format!(
                    "worn items' skills and kits read: g_i_belt008 Stealth {:?}, stealth unit {:?}; g_i_trapkit004 lays row {:?}",
                    unit.map(|f| f.gear.skills[2]),
                    unit.map(|f| f.stealth_unit),
                    kit.and_then(|f| f.trap)
                ),
            );
        }
        // K1's items with properties beyond combat, as their templates have
        // them (journal 2026-10-05-item-properties); a crystal's fitted.
        if game == vfs::Game::Kotor1 {
            use sim::combat::gear::{Against, sides, traits};
            use sim::inventory::Limit;
            let none = sim::inventory::Catalog::default();
            let of = |name: &str, fitted: u32| {
                templates
                    .get(name)
                    .map(|facts| facts.fitted(fitted, &[], &none))
            };
            type Reads = fn(&sim::inventory::ItemFacts) -> bool;
            let read: [(&str, u32, Reads); 9] = [
                ("ptar_sitharmor", 0, |f| f.gear.disguise == Some(28)),
                ("g_i_glowrod01", 0, |f| f.gear.has(traits::LIGHT)),
                ("g_a_mstrrobe06", 0, |f| f.gear.regenerate == 1),
                ("g1_i_implant302", 0, |f| f.gear.immune_to(1)),
                ("g1_i_implant303", 0, |f| [32, 35, 39].iter().all(|feat| f.gear.feats.has(*feat))),
                ("g_a_class4001", 0, |f| f.limits.contains(&Limit::Race(6))),
                ("g1_i_drdshld001", 0, |f| f.limits.contains(&Limit::Feat(78))),
                ("g_w_lghtsbr01", 1 << 24, |f| {
                    f.limits.contains(&Limit::Side(sides::LIGHT))
                        && f.weapon.and_then(|w| w.versus).map(|v| v.against)
                            == Some(Against::Side(sides::DARK))
                }),
                ("g1_w_lghtsbr01", 1 << 10, |f| {
                    f.weapon.is_some_and(|w| w.on_hit[0].is_some())
                }),
            ];
            let seen: Vec<String> = read
                .iter()
                .map(|(name, fitted, ok)| {
                    let facts = of(name, *fitted);
                    let mark = if facts.as_ref().is_some_and(*ok) {
                        "ok"
                    } else {
                        "MISSING"
                    };
                    format!("{name} {mark}")
                })
                .collect();
            report.check(
                seen.iter().all(|line| line.ends_with(" ok")),
                format!("item properties beyond combat read: {}", seen.join(", ")),
            );
        }
        let armed: Vec<(u32, &sim::module::ObjectData)> = ids
            .iter()
            .copied()
            .filter(|&(id, _)| state.in_slot(id, slots::RIGHT_WEAPON).is_some())
            .collect();
        let dressed = armed.iter().all(|&(id, object)| {
            let item = state
                .in_slot(id, slots::RIGHT_WEAPON)
                .and_then(|item| state.items.get(&item.0))
                .and_then(|item| templates.get(&item.template));
            object.sheet.map(|sheet| sheet.weapons[0]) == item.map(|facts| facts.weapon)
        });
        report.check(
            dressed && !armed.is_empty(),
            format!(
                "{} armed creatures' sheets carry the weapon their right hand holds",
                armed.len()
            ),
        );
        let creature = snapshot
            .entities
            .iter()
            .find(|e| matches!(e.kind, sim::Kind::Object(_, at) if at as usize == index))
            .map(|e| e.id.0)
            .ok_or("the creature")?;
        let was = objects.get(index as u32).and_then(|object| object.sheet);
        let was = was.ok_or("no sheet")?;
        let held = state
            .in_slot(creature, slots::RIGHT_WEAPON)
            .ok_or("no weapon held")?;
        let old_armour = state
            .in_slot(creature, slots::BODY)
            .and_then(|item| state.items.get(&item.0))
            .and_then(|item| templates.get(&item.template))
            .and_then(|facts| facts.armour)
            .map_or(0, |armour| armour.defence);

        probe.queue("kjk_items1", probe.me);
        probe.step(2);
        let state = probe.world.script_state().clone();
        let stacks: Vec<u16> = state
            .items
            .values()
            .filter(|item| item.holder == PARTY && item.template == medpac.template)
            .map(|item| item.stack)
            .collect();
        report.check(
            stacks == [2] && state.gold == 100,
            format!(
                "two {} made on the player: the party's stacks {stacks:?}; gold {} (150 given, 50 taken)",
                medpac.template, state.gold
            ),
        );

        probe.queue("kjk_items2", sim::SimId(creature));
        let after = probe.step(2);
        let theirs = after.get(sim::SimId(creature)).ok_or("the creature")?;
        let sheet = theirs.sheet.ok_or("no sheet in the snapshot")?;
        let defence = was.defence - old_armour + suit.armour.map_or(0, |a| a.defence);
        report.check(
            sheet.weapons[0] == blade.weapon && sheet.defence == defence,
            format!(
                "the creature equips {} and {}: its sheet in the snapshot, weapon base item {:?}, defence {} → {}",
                blade.template,
                suit.template,
                sheet.weapons[0].map(|w| w.base_item),
                was.defence,
                sheet.defence
            ),
        );
        let state = probe.world.script_state();
        report.check(
            state
                .items
                .get(&held.0)
                .is_some_and(|item| item.holder == creature && item.slot.is_none()),
            "the weapon it held is in its inventory".to_string(),
        );

        probe.queue("kjk_items3", probe.me);
        probe.step(2);
        let state = probe.world.script_state().clone();
        report.check(
            !state
                .items
                .values()
                .any(|item| item.holder == PARTY && item.template == medpac.template),
            "DestroyObject on GetItemPossessedBy's stack: gone".to_string(),
        );
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: probe.world.level().id(),
            snapshot: probe.world.snapshot(),
            script: state.clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        report.check(
            loaded.script.items == state.items && loaded.script.gold == state.gold,
            format!(
                "a save carries the {} items and the gold",
                state.items.len()
            ),
        );
        let faults = probe.faults();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        Ok(json!({ "module": module, "stock": stock }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
