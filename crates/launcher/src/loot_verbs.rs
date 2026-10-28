//! `kjk script loot <game> [module]`: what a module's creatures will leave
//! in their remains (`sim::remains`) and what its containers hold, checked
//! against the games' rules. The module is installed with its own scripts
//! and stepped past one heartbeat, so their spawn and heartbeat scripts
//! have run (K1's `k_inc_drop` from `GN_SetListeningPatterns`, TSL's
//! `PlaceCritterTreasure` in `k_def_spawn01`, K1's civilian footlockers'
//! `k_plc_tresciv`):
//!
//! 1. An item a creature's template lists drops only when the list marks
//!    it `Dropable` (both engines load a missing mark as 0); worn or
//!    carried alike.
//! 2. What its scripts made on it drops (`CreateItemOnObject`), unless it
//!    joined a stack of an unmarked listed item (the engines' merge keeps
//!    the stack's flag).
//! 3. The routines the treasure scripts call are bound, not gaps.
//! 4. A container holds what its template lists and what its scripts
//!    gave it, all of it what a player opening it is shown and may take
//!    (`State::loot`): each one holding anything is opened by the player
//!    stood beside it, the loot window's list (the snapshot's goods for
//!    it) read, and everything taken into the party's items and gold.

use std::collections::BTreeMap;
use std::process::ExitCode;

use serde_json::json;
use sim::action::Action;
use vfs::{Discovery, Game};

use crate::probe::Probe;
use crate::scenes::Report;
use crate::script_verbs::{Stepped, step_module};

/// How long the module is stepped: its creatures' spawn scripts done and
/// every object's heartbeat run once.
const SECONDS: u64 = rules::world::timings::HEARTBEAT_MS / 1000 + 1;

/// The routines K1's `k_inc_drop` and TSL's `k_inc_treas_k2` call.
const TREASURE_ROUTINES: [&str; 17] = [
    "Random",
    "GetRacialType",
    "GetStandardFaction",
    "GetSubRace",
    "GetHitDice",
    "CreateItemOnObject",
    "GetGlobalNumber",
    "SetGlobalNumber",
    "GetLocalBoolean",
    "SetLocalBoolean",
    "IsAvailableCreature",
    "GetModuleName",
    "GetAbilityModifier",
    "FindSubString",
    "GetSubString",
    "StringToInt",
    "IntToString",
];

pub fn loot(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let game = Game::from_key(game).ok_or(format!("{game:?}: use k1 or tsl"))?;
        let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
        let installer = crate::lifecycle::Installer::new(discovery, game, anims)?;
        let options = crate::sweep_verbs::coverage(SECONDS);
        let Stepped {
            mut world, level, ..
        } = step_module(&installer, module, &options)?;
        let snapshot = world.snapshot();
        let state = world.script_state();
        let objects = sim::Instances::in_snapshot(&level, &snapshot);
        let mut wrong = Vec::new();
        let mut creatures = Vec::new();
        let mut given_to = 0;
        for entity in &snapshot.entities {
            let sim::Kind::Object(sim::module::ObjectKind::Creature, _) = entity.kind else {
                continue;
            };
            let Some(facts) = objects.of_entity(entity) else {
                continue;
            };
            // Units by template: listed, marked, held, held droppable.
            let mut units: BTreeMap<&str, [u32; 4]> = BTreeMap::new();
            for held in &facts.inventory {
                let counts = units.entry(held.template.as_str()).or_default();
                counts[0] += u32::from(held.stack);
                counts[1] += u32::from(held.stack) * u32::from(held.droppable);
            }
            let held: Vec<&sim::inventory::Item> = state
                .items
                .values()
                .filter(|item| item.holder == entity.id.0)
                .collect();
            for item in &held {
                let counts = units.entry(item.template.as_str()).or_default();
                counts[2] += u32::from(item.stack);
                counts[3] += u32::from(item.stack) * u32::from(item.droppable);
            }
            let names = |pick: &dyn Fn(&sim::inventory::Item) -> bool| -> Vec<String> {
                held.iter()
                    .filter(|item| pick(item))
                    .map(|item| format!("{} ×{}", item.template, item.stack))
                    .collect()
            };
            let drops = names(&|item| item.droppable);
            let keeps = names(&|item| !item.droppable);
            let mut given = Vec::new();
            for (template, [listed, marked, had, dropped]) in &units {
                // What scripts made: the units beyond the lists'. One joining
                // an unmarked stack stays with it (the engines' merge).
                let made = had.saturating_sub(*listed);
                if made > 0 {
                    given.push(format!("{template} ×{made}"));
                }
                if *dropped > marked + made {
                    wrong.push(format!("{}: {template} drops unmarked", facts.template));
                }
                if *listed == 0 && dropped < had {
                    wrong.push(format!("{}: {template}, made by a script, stays", facts.template));
                }
            }
            if !given.is_empty() {
                given_to += 1;
            }
            outln!(
                "  {} ({}): drops [{}]; keeps [{}]; scripts gave [{}]",
                facts.template,
                facts.tag,
                drops.join(", "),
                keeps.join(", "),
                given.join(", ")
            );
            creatures.push(json!({
                "template": facts.template,
                "tag": facts.tag,
                "drops": drops,
                "keeps": keeps,
                "given": given,
            }));
        }
        report.check(
            wrong.is_empty() && !creatures.is_empty(),
            format!(
                "{} creatures: a listed item drops only when its list marks it, what scripts made drops{}",
                creatures.len(),
                if wrong.is_empty() {
                    String::new()
                } else {
                    format!(": {}", wrong.join("; "))
                }
            ),
        );
        let report_run = world.script_report();
        let gaps: Vec<&str> = TREASURE_ROUTINES
            .into_iter()
            .filter(|name| report_run.gaps.contains_key(*name))
            .collect();
        report.check(
            gaps.is_empty(),
            format!(
                "the treasure scripts' routines are bound (gaps {gaps:?}); {given_to} creatures given treasure by their scripts"
            ),
        );
        let faults = &report_run.faults;
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        let containers = containers(report, &snapshot, &objects, state);
        let opened = opened(report, discovery, game, module)?;
        Ok(json!({
            "game": game.key(),
            "module": module,
            "creatures": creatures,
            "containers": containers,
            "opened": opened,
        }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// The module's containers: what each one's template lists and what it
/// holds that a player opening it is shown (`State::loot`), the scripts
/// that gave the rest named by their hooks. Checked: what it lists it
/// holds, untouched as nothing has opened it yet.
fn containers(
    report: &mut Report,
    snapshot: &sim::Snapshot,
    objects: &sim::Instances,
    state: &sim::script::State,
) -> Vec<serde_json::Value> {
    let mut listed_missing = Vec::new();
    let mut out = Vec::new();
    let (mut filled, mut empty) = (0, 0);
    for entity in &snapshot.entities {
        let sim::Kind::Object(sim::module::ObjectKind::Placeable, _) = entity.kind else {
            continue;
        };
        let Some(facts) = objects.of_entity(entity).filter(|facts| facts.container) else {
            continue;
        };
        let mut units: BTreeMap<&str, [u32; 2]> = BTreeMap::new();
        for held in &facts.inventory {
            units.entry(held.template.as_str()).or_default()[0] += u32::from(held.stack);
        }
        let shown: Vec<String> = (state.loot(entity.id.0))
            .map(|(_, item)| {
                units.entry(item.template.as_str()).or_default()[1] += u32::from(item.stack);
                format!("{} ×{}", item.template, item.stack)
            })
            .collect();
        let given: Vec<String> = (units.iter())
            .filter(|(_, [listed, held])| held > listed)
            .map(|(template, [listed, held])| format!("{template} ×{}", held - listed))
            .collect();
        if units.values().any(|[listed, held]| held < listed) {
            listed_missing.push(facts.template.clone());
        }
        if shown.is_empty() {
            empty += 1;
        } else {
            filled += 1;
        }
        let hooks: Vec<String> = (facts.scripts.iter())
            .map(|(hook, script)| format!("{hook}={script}"))
            .collect();
        outln!(
            "  {} ({}) [{}]: lists {}; shows [{}]; scripts gave [{}]",
            facts.template,
            facts.tag,
            hooks.join(" "),
            facts.inventory.len(),
            shown.join(", "),
            given.join(", ")
        );
        out.push(json!({
            "template": facts.template,
            "tag": facts.tag,
            "hooks": hooks,
            "listed": facts.inventory.len(),
            "shows": shown,
            "given": given,
        }));
    }
    report.check(
        listed_missing.is_empty(),
        format!(
            "{} containers: {filled} show items, {empty} empty; each holds what it lists{}",
            out.len(),
            if listed_missing.is_empty() {
                String::new()
            } else {
                format!(" (not: {})", listed_missing.join(", "))
            }
        ),
    );
    out
}

/// How far east of a container the player stands to open it (metres).
const BESIDE: f32 = 1.0;

/// Each container holding anything, in the module as the game has it
/// stepped as long as `loot`'s, opened by the player stood beside it: what
/// the loot window lists (the snapshot's goods for it, `Goods::carried`)
/// is everything it holds, and a take all gives it all to the party.
fn opened(
    report: &mut Report,
    discovery: &Discovery,
    game: Game,
    module: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let mut probe = Probe::own(discovery, game, module)?;
    let start = probe.step(SECONDS * u64::from(sim::TICK_HZ));
    let level = probe.world.level().clone();
    let objects = sim::Instances::in_snapshot(&level, &start);
    let boxes: Vec<(u32, sim::SimId, [f32; 3])> = (start.entities.iter())
        .filter_map(|entity| {
            let sim::Kind::Object(sim::module::ObjectKind::Placeable, index) = entity.kind else {
                return None;
            };
            let facts = objects.of_entity(entity).filter(|facts| facts.container)?;
            Some((index, entity.id, facts.position))
        })
        .filter(|(_, id, _)| probe.world.script_state().loot(id.0).next().is_some())
        .collect();
    let units = |items: &mut dyn Iterator<Item = (u32, &sim::inventory::Item)>| {
        let mut listed: Vec<String> = items
            .map(|(_, item)| format!("{} ×{}", item.template, item.stack))
            .collect();
        listed.sort();
        listed
    };
    let (mut wrong, mut out) = (Vec::new(), Vec::new());
    for (index, id, at) in boxes {
        let holds = units(&mut probe.world.script_state().loot(id.0));
        probe.place(at[0] + BESIDE, at[1])?;
        let (open, told) = probe.act(Action::Open(index));
        let shown = units(&mut open.goods.carried(id.0));
        let tag = objects.get(index).map_or("", |facts| facts.tag.as_str());
        if shown.is_empty() {
            // A locked one stays shut: what it holds is not shown.
            outln!("  opened {tag} (#{index}): not opened ({told:?})");
            continue;
        }
        let carried = |goods: &sim::inventory::Goods| -> u32 {
            (goods.carried(sim::inventory::PARTY))
                .map(|(_, item)| u32::from(item.stack))
                .sum()
        };
        let (gold, party) = (open.goods.gold, carried(&open.goods));
        let (after, _) = probe.act(Action::TakeAll(index));
        let left = probe.world.script_state().loot(id.0).count();
        let gained = (after.goods.gold - gold, carried(&after.goods) - party);
        probe.act(Action::Close(index));
        if shown != holds || left != 0 {
            wrong.push(format!("{tag}: shows {shown:?}, holds {holds:?}, {left} left"));
        }
        outln!(
            "  opened {tag} (#{index}): the window lists [{}]; taken: {} credits and {} items into the party's, {left} left",
            shown.join(", "),
            gained.0,
            gained.1
        );
        out.push(json!({ "tag": tag, "index": index, "shown": shown, "credits": gained.0, "items": gained.1 }));
    }
    report.check(
        wrong.is_empty(),
        format!(
            "{} containers opened: each lists all it holds and gives it all{}",
            out.len(),
            if wrong.is_empty() {
                String::new()
            } else {
                format!(" (not: {})", wrong.join("; "))
            }
        ),
    );
    Ok(out)
}
