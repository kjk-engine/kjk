//! `kjk combat encounters`: the encounter overrides' evidence (I28). The
//! module is planned as the game has it and with its overrides
//! (`data/encounters/<game>/<module>.encounter`) for one to `players`
//! players; a table shows every creature's maximum HP (or its absence)
//! before and after for each count, with the hostiles' count and total HP,
//! and checks that every change matched a creature and that each plan
//! installs (every seat has room where it arrives). Writes `facts.json`.

use std::process::ExitCode;

use sim::combat::sheet::factions;
use sim::module::{ModuleData, ObjectKind};
use vfs::Discovery;

/// A module's creatures: (tag, max HP, hostile to the player), in object
/// order.
fn creatures(data: &ModuleData) -> Vec<(String, u16, bool)> {
    data.objects()
        .filter(|object| object.kind == ObjectKind::Creature)
        .map(|object| {
            let sheet = object.sheet.unwrap_or_else(sim::combat::Sheet::commoner);
            (
                object.tag.clone(),
                sheet.max_hp,
                data.repute.hostile(sheet.faction, factions::PLAYER),
            )
        })
        .collect()
}

/// `kjk combat encounters`.
pub fn encounters(discovery: &Discovery, game: &str, module: &str, players: u8) -> ExitCode {
    let installer = match crate::scenes::installer(discovery, game) {
        Ok(installer) => installer,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let before = match installer.plan_without_encounters(&installer.request(module, None, 1)) {
        Ok(plan) => creatures(plan.level.module().expect("a plan has its module")),
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut afters = Vec::new();
    let mut failures = Vec::new();
    for count in 1..=players.max(1) {
        let request = installer.request(module, None, count).with_players(count);
        match installer.prepare(&request, false) {
            Ok(prepared) => {
                let applied = prepared.plan.facts.encounter.clone();
                if !applied.unmatched.is_empty() {
                    failures.push(format!(
                        "{count} player(s): lines {:?} matched no creature",
                        applied.unmatched
                    ));
                }
                afters.push((
                    count,
                    creatures(prepared.plan.level.module().expect("a plan has its module")),
                    applied,
                ));
            }
            Err(error) => failures.push(format!("{count} player(s): {error}")),
        }
    }
    outln!("combat encounters {game} {module}: max HP before and with the overrides, by players");
    let mut header = format!("  {:<22} {:>6}", "creature", "before");
    for (count, ..) in &afters {
        header.push_str(&format!(" {:>6}", format!("{count}p")));
    }
    outln!("{header}");
    let rows = afters
        .iter()
        .map(|(_, list, _)| list.len())
        .max()
        .unwrap_or(0)
        .max(before.len());
    for row in 0..rows {
        let tag = afters
            .iter()
            .find_map(|(_, list, _)| list.get(row).map(|(tag, ..)| tag.clone()))
            .or_else(|| before.get(row).map(|(tag, ..)| tag.clone()))
            .unwrap_or_else(|| "-".to_string());
        let mut line = format!(
            "  {:<22} {:>6}",
            tag,
            before
                .get(row)
                .map_or_else(|| "-".to_string(), |(_, hp, _)| hp.to_string())
        );
        for (_, list, _) in &afters {
            line.push_str(&format!(
                " {:>6}",
                list.get(row)
                    .map_or_else(|| "-".to_string(), |(_, hp, _)| hp.to_string())
            ));
        }
        outln!("{line}");
    }
    let hostile = |list: &[(String, u16, bool)]| {
        let hostiles: Vec<u32> = list
            .iter()
            .filter(|(_, _, hostile)| *hostile)
            .map(|(_, hp, _)| u32::from(*hp))
            .collect();
        (hostiles.len(), hostiles.iter().sum::<u32>())
    };
    let (count, hp) = hostile(&before);
    let mut summary = format!("  hostiles: before {count} ({hp} HP)");
    for (players, list, applied) in &afters {
        let (count, hp) = hostile(list);
        summary.push_str(&format!(
            "; {players}p {count} ({hp} HP, {} changes, {} skipped)",
            applied.applied, applied.skipped
        ));
    }
    outln!("{summary}");
    for failure in &failures {
        outln!("  FAIL {failure}");
    }
    crate::runtime::write_facts(serde_json::json!({
        "verb": "combat encounters", "game": game, "module": module,
        "before": before.iter().map(|(tag, hp, hostile)| serde_json::json!({"tag": tag, "hp": hp, "hostile": hostile})).collect::<Vec<_>>(),
        "after": afters.iter().map(|(players, list, applied)| serde_json::json!({
            "players": players,
            "creatures": list.iter().map(|(tag, hp, hostile)| serde_json::json!({"tag": tag, "hp": hp, "hostile": hostile})).collect::<Vec<_>>(),
            "applied": applied.applied, "skipped": applied.skipped, "unmatched": applied.unmatched,
        })).collect::<Vec<_>>(),
        "failures": failures,
    }));
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
