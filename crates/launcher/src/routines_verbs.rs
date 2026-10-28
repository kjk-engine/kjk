//! `kjk script routines <game> [module]`: the routines the whole-game
//! sweep found most reached, checked end to end. Two scripts made here
//! (`probe`) run as the player in the module (the Endar Spire by default)
//! on its first creature with a sheet and a name:
//!
//! 1. The module's and the area's names, the creature's name, a talk-table
//!    line, an object as hex, a vector's length; the creature made hostile
//!    and kept at one HP at least; its alignment read as its template has
//!    it, set to 80, shifted 15 toward neutral and 40 toward the dark side
//!    (25: dark). TSL's own routines too: a number global
//!    raised and lowered within a signed byte, and past it (the sum
//!    refused); the creature's personal space; the party leader; its
//!    heartbeat taken away.
//!    An item made on the player (the module's first whose tag has
//!    capitals, else its first) answers `GetTag` with its tag as its data
//!    writes it.
//! 2. The creature no longer kept at one HP, and its combat cancelled; a
//!    save carries the heartbeat taken away and the alignment.
//! 3. The player standing at 0.6 HP is alive at 1 HP to the scripts; the
//!    nearest creature dead is left out of `GetNearestCreature` unless it
//!    asks for the dead (`CREATURE_TYPE_IS_ALIVE`).

use std::process::ExitCode;

use serde_json::json;
use sim::combat::sheet::flags;
use vfs::Discovery;

use crate::probe::{Call, Probe, by_tag, float, int, me, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// `Set<kind>(name, <value>)`: the value's calls, then the name.
fn keep(setter: &'static str, name: &str, value: Vec<Call>) -> Vec<Call> {
    let mut calls = value;
    calls.push(text(name));
    calls.push(Call::Routine(setter, 2));
    calls
}

fn number(name: &str, value: Vec<Call>) -> Vec<Call> {
    keep("SetGlobalNumber", name, value)
}

fn string(name: &str, value: Vec<Call>) -> Vec<Call> {
    keep("SetGlobalString", name, value)
}

/// `routine(<object>, <arguments>)`: the arguments' calls (last first),
/// then the object's.
fn on(routine: &'static str, object: &[Call], arguments: &[Call]) -> Vec<Call> {
    let mut calls = arguments.to_vec();
    calls.extend_from_slice(object);
    calls.push(Call::Routine(routine, 1 + arguments.len() as u8));
    calls
}

/// `IncrementGlobalNumber` or `DecrementGlobalNumber` of `name` by `by`.
fn change(routine: &'static str, name: &str, by: i32) -> Vec<Call> {
    vec![int(by), text(name), Call::Routine(routine, 2)]
}

/// `FloatToString(<value>, 0, 2)`.
fn two_places(value: Vec<Call>) -> Vec<Call> {
    let mut calls = vec![int(2), int(0)];
    calls.extend(value);
    calls.push(Call::Routine("FloatToString", 3));
    calls
}

/// A talk-table line both games have.
const STRREF: i32 = 1;

pub fn routines(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let talk = base.talk.get().ok_or("no dialog.tlk")?;
        let constant = |name: &str| {
            routines
                .constant_named(name)
                .and_then(|c| match c.value {
                    script_ncs::catalog::Value::Int(v) => Some(v),
                    _ => None,
                })
                .ok_or(format!("no {name}"))
        };
        let hostile_1 = constant("STANDARD_FACTION_HOSTILE_1")?;
        let [neutral, dark] = [
            constant("ALIGNMENT_NEUTRAL")?,
            constant("ALIGNMENT_DARK_SIDE")?,
        ];
        // `GetNearestCreature(CREATURE_TYPE_PLAYER_CHAR, PLAYER_CHAR_NOT_PC,
        // me, 1, <second>)` as a hex string.
        let not_pc = [
            constant("CREATURE_TYPE_PLAYER_CHAR")?,
            constant("PLAYER_CHAR_NOT_PC")?,
        ];
        let nearest = |second: [i32; 2]| {
            let mut calls = [-1, -1, second[1], second[0], 1].map(int).to_vec();
            calls.extend([me(), int(not_pc[1]), int(not_pc[0])]);
            calls.push(Call::Routine("GetNearestCreature", 8));
            on("ObjectToString", &calls, &[])
        };
        let dead_only = [constant("CREATURE_TYPE_IS_ALIVE")?, constant("FALSE")?];
        // TSL's own: number globals raised and lowered, personal space,
        // the party leader, heartbeats taken away.
        let tsl = routines.routine_named("IncrementGlobalNumber").is_some();
        let mut chosen = None;
        let mut item_tag = None;
        let mut probe = Probe::new(discovery, game, module, |level| {
            let data = level.module().ok_or("no module")?;
            let mut seen = std::collections::BTreeMap::new();
            for object in data.objects() {
                if object.kind == sim::module::ObjectKind::Creature {
                    let nth = seen.entry(object.tag.clone()).or_insert(0);
                    if chosen.is_none() && object.sheet.is_some() && !object.name.is_empty() {
                        chosen = Some((object.tag.clone(), *nth, object.clone().into_owned()));
                    }
                    *nth += 1;
                }
            }
            let (tag, nth, _) = chosen.as_ref().ok_or("no named creature with a sheet")?;
            let creature = by_tag(tag, *nth);
            let items = level
                .scripts()
                .ok_or("the module has no scripts")?
                .items()
                .clone();
            let item = items
                .iter()
                .find(|facts| !facts.cased_tag.is_empty())
                .or_else(|| items.iter().next())
                .ok_or("the module has no items")?;
            item_tag = Some(item.tag.clone());
            let made = vec![
                int(1),
                me(),
                text(&item.template),
                Call::Routine("CreateItemOnObject", 3),
            ];
            let mut first = vec![
                string("kjk_module", vec![Call::Routine("GetModuleName", 0)]),
                string("kjk_area", on("GetName", &on("GetArea", &[me()], &[]), &[])),
                string("kjk_name", on("GetName", &creature, &[])),
                string(
                    "kjk_line",
                    vec![int(STRREF), Call::Routine("GetStringByStrRef", 1)],
                ),
                string(
                    "kjk_module_hex",
                    on("ObjectToString", &[Call::Routine("GetModule", 0)], &[]),
                ),
                string(
                    "kjk_length",
                    two_places(vec![
                        float(0.0),
                        float(4.0),
                        float(3.0),
                        Call::Routine("Vector", 3),
                        Call::Routine("VectorMagnitude", 1),
                    ]),
                ),
                string("kjk_item_tag", on("GetTag", &made, &[])),
                on("ChangeToStandardFaction", &creature, &[int(hostile_1)]),
                on("SetMinOneHP", &creature, &[int(1)]),
                number("kjk_min1", on("GetMinOneHP", &creature, &[])),
                number("kjk_good_evil_read", on("GetGoodEvilValue", &creature, &[])),
                on("SetGoodEvilValue", &creature, &[int(80)]),
                on("AdjustAlignment", &creature, &[int(15), int(neutral)]),
                on("AdjustAlignment", &creature, &[int(40), int(dark)]),
                number("kjk_good_evil", on("GetGoodEvilValue", &creature, &[])),
                number("kjk_side", on("GetAlignmentGoodEvil", &creature, &[])),
            ];
            if tsl {
                first.extend([
                    number("kjk_n", vec![int(120)]),
                    change("IncrementGlobalNumber", "kjk_n", 5),
                    change("IncrementGlobalNumber", "kjk_n", 5),
                    number(
                        "kjk_raised",
                        vec![text("kjk_n"), Call::Routine("GetGlobalNumber", 1)],
                    ),
                    change("DecrementGlobalNumber", "kjk_n", 300),
                    change("DecrementGlobalNumber", "kjk_n", 10),
                    string(
                        "kjk_space",
                        two_places(on("GetObjectPersonalSpace", &creature, &[])),
                    ),
                    string(
                        "kjk_leader",
                        on("ObjectToString", &[Call::Routine("GetPartyLeader", 0)], &[]),
                    ),
                    number("kjk_leads", on("GetIsPartyLeader", &[me()], &[])),
                    on("RemoveHeartbeat", &creature, &[]),
                ]);
            }
            let second = vec![
                on("SetMinOneHP", &creature, &[int(0)]),
                number("kjk_min1_after", on("GetMinOneHP", &creature, &[])),
                on("CancelCombat", &creature, &[]),
            ];
            let third = vec![
                number("kjk_dead", on("GetIsDead", &[me()], &[])),
                number("kjk_hp", on("GetCurrentHitPoints", &[me()], &[])),
                string("kjk_nearest", nearest([-1, -1])),
                string("kjk_nearest_dead", nearest(dead_only)),
            ];
            let mut scripts = std::collections::BTreeMap::new();
            for (name, calls) in [
                ("kjk_routines1", &first),
                ("kjk_routines2", &second),
                ("kjk_routines3", &third),
            ] {
                scripts.insert(name.to_string(), crate::probe::program(&routines, calls)?);
            }
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_talk(talk.clone())
                    .with_items(items),
            )
        })?;
        let (tag, nth, facts) = chosen.ok_or("no creature chosen")?;
        let data = probe.world.level().module().ok_or("no module")?.clone();
        let snapshot = probe.world.snapshot();
        let creature = crate::scenes::tagged(probe.world.level(), &snapshot, &tag)
            .get(nth as usize)
            .map(|&at| snapshot.entities[at].id)
            .ok_or("the chosen creature")?;
        let numbers = |probe: &Probe, name: &str| {
            let state = probe.world.script_state();
            sim::script::global(&state.numbers, name).copied().unwrap_or(-1)
        };
        let strings = |probe: &Probe, name: &str| {
            let state = probe.world.script_state();
            sim::script::global(&state.strings, name).cloned().unwrap_or_else(String::new)
        };

        probe.queue("kjk_routines1", probe.me);
        let after = probe.step(2);
        if tsl {
            report.check(
                numbers(&probe, "kjk_raised") == 125 && numbers(&probe, "kjk_n") == 115,
                format!(
                    "a number global: 120 + 5 = 125, + 5 refused (past 127), − 300 refused, − 10 = {}",
                    numbers(&probe, "kjk_n")
                ),
            );
        }
        let module_name = strings(&probe, "kjk_module");
        report.check(
            !module_name.is_empty() && module_name == data.title,
            format!("GetModuleName: {module_name:?} (the IFO's Mod_Name as shown)"),
        );
        let area = strings(&probe, "kjk_area");
        report.check(
            area == data.area_title,
            format!("the area's name: {area:?}"),
        );
        let name = strings(&probe, "kjk_name");
        report.check(name == facts.name, format!("{tag}: named {name:?}"));
        if tsl {
            let space = strings(&probe, "kjk_space");
            report.check(
                space == format!("{:.2}", facts.personal_space) && facts.personal_space > 0.0,
                format!("{tag}: personal space {space} m"),
            );
        }
        let line: String = talk
            .text(STRREF as u32)
            .unwrap_or(&[])
            .iter()
            .map(|&byte| char::from(byte))
            .collect();
        report.check(
            strings(&probe, "kjk_line") == line,
            format!("GetStringByStrRef({STRREF}): {line:?}"),
        );
        report.check(
            strings(&probe, "kjk_module_hex") == "7e000001",
            format!(
                "ObjectToString(GetModule()): {:?}",
                strings(&probe, "kjk_module_hex")
            ),
        );
        if tsl {
            let me_hex = format!("{:x}", probe.me.0);
            report.check(
                strings(&probe, "kjk_leader") == me_hex && numbers(&probe, "kjk_leads") == 1,
                format!("the party leader is the player ({me_hex})"),
            );
        }
        let item_tag = item_tag.ok_or("no item chosen")?;
        report.check(
            strings(&probe, "kjk_item_tag") == item_tag,
            format!(
                "an item's GetTag lowercased, as the games load it: {:?} ({item_tag:?})",
                strings(&probe, "kjk_item_tag")
            ),
        );
        report.check(
            strings(&probe, "kjk_length") == "5.00",
            format!(
                "VectorMagnitude(Vector(3, 4, 0)): {}",
                strings(&probe, "kjk_length")
            ),
        );
        let entity = after.get(creature).ok_or("the creature")?;
        let fighter = entity.fighter.ok_or("a fighter")?;
        let sheet = entity.sheet.ok_or("its sheet in the snapshot")?;
        report.check(
            fighter.faction.map(i32::from) == Some(hostile_1)
                && sheet.flags & flags::MIN_ONE_HP != 0
                && numbers(&probe, "kjk_min1") == 1,
            format!(
                "{tag} made hostile (faction {:?}) and kept at one HP at least",
                fighter.faction
            ),
        );
        let read = facts.sheet.map_or(-2, |sheet| i32::from(sheet.good_evil));
        report.check(
            numbers(&probe, "kjk_good_evil_read") == read
                && numbers(&probe, "kjk_good_evil") == 25
                && numbers(&probe, "kjk_side") == dark
                && sheet.good_evil == 25,
            format!(
                "{tag}'s alignment: {} as its template has it ({read}); 80, 15 toward neutral, 40 toward dark: {} (side {})",
                numbers(&probe, "kjk_good_evil_read"),
                numbers(&probe, "kjk_good_evil"),
                numbers(&probe, "kjk_side")
            ),
        );
        if tsl {
            let heartless = probe.world.script_state().heartless.contains(&creature.0);
            report.check(heartless, format!("{tag}'s heartbeat taken away"));
        }

        probe.queue("kjk_routines2", probe.me);
        let after = probe.step(1);
        let entity = after.get(creature).ok_or("the creature")?;
        let fighter = entity.fighter.ok_or("a fighter")?;
        let sheet = entity.sheet.ok_or("its sheet in the snapshot")?;
        report.check(
            sheet.flags & flags::MIN_ONE_HP == 0
                && numbers(&probe, "kjk_min1_after") == 0
                && fighter.target.is_none()
                && fighter.combat_until <= after.time_ms,
            format!("{tag}: no longer kept at one HP; its combat cancelled"),
        );

        // 3. The player standing with part of a point (JA's shares of damage
        // leave hundredths) is alive to the scripts at 1 HP, so their attacks
        // still go at it; the dead are no one's nearest creature unless the
        // script asks for them.
        probe.edit_me(|me| {
            me.fighter.as_mut().ok_or("no fighter")?.hp = 60;
            Ok(())
        })?;
        probe.queue("kjk_routines3", probe.me);
        probe.step(1);
        report.check(
            numbers(&probe, "kjk_dead") == 0 && numbers(&probe, "kjk_hp") == 1,
            format!(
                "standing at 0.6 HP: GetIsDead {}, GetCurrentHitPoints {}",
                numbers(&probe, "kjk_dead"),
                numbers(&probe, "kjk_hp")
            ),
        );
        let nearest = strings(&probe, "kjk_nearest");
        let id = u32::from_str_radix(&nearest, 16).map_err(|e| format!("{nearest}: {e}"))?;
        probe.edit(sim::SimId(id), |them| {
            let fighter = them.fighter.as_mut().ok_or("no fighter")?;
            fighter.hp = 0;
            fighter.life = sim::combat::fighter::Life::Dead;
            Ok(())
        })?;
        probe.queue("kjk_routines3", probe.me);
        probe.step(1);
        let (after, dead) = (
            strings(&probe, "kjk_nearest"),
            strings(&probe, "kjk_nearest_dead"),
        );
        report.check(
            after != nearest && dead == nearest,
            format!(
                "GetNearestCreature: {nearest} dead, the nearest is {after}; asked for the dead, {dead}"
            ),
        );

        let state = probe.world.script_state().clone();
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: probe.world.level().id(),
            snapshot: probe.world.snapshot(),
            script: state.clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        report.check(
            loaded.script.heartless == state.heartless,
            format!(
                "a save carries the heartbeats taken away ({})",
                state.heartless.len()
            ),
        );
        let kept = loaded
            .snapshot
            .get(creature)
            .and_then(|entity| entity.sheet)
            .map(|sheet| sheet.good_evil);
        report.check(
            kept == Some(25),
            format!("a save carries {tag}'s alignment ({kept:?})"),
        );
        let faults = probe.faults();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        Ok(json!({ "module": module, "creature": tag, "module_name": module_name }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
