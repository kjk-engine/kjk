//! `kjk remote census k1|tsl`: the duel census of the remote droid
//! (`docs/superpowers/specs/2026-10-29-remote-droid-design.md`, "How we
//! prove it's safe" 2). Every module's own scripts read for a scripted
//! round: a script that turns someone hostile (`ChangeToStandardFaction`,
//! `ChangeFaction`, `ChangeFactionByFaction`) in a module whose scripts end
//! a fight themselves (`SurrenderToEnemies`, `SurrenderRetainBuffs`,
//! `SurrenderByFaction`, `CancelCombat`). Such a round is covered when the
//! module also protects a fighter the way the droid's gate reads
//! (`remote::gate`): its scripts call `SetMinOneHP` or `SetPlotFlag`, or a
//! creature template of its own is made Min1HP or plot; a round that is not is
//! listed `[verify]`, for the chaos chain (`kjk chain --droids chaos`) and
//! a person to judge. A static read: which object each call names is the
//! script's own business, so this proves shape, not identity.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;
use fmt_odyssey::ncs::{Ncs, Op};
use serde_json::json;
use vfs::resolver::Resolver;
use vfs::Discovery;

use crate::script_verbs::{catalog, ncs_type, open};

/// The routines that turn a creature on the party.
const HOSTILE: [&str; 3] = [
    "ChangeToStandardFaction",
    "ChangeFaction",
    "ChangeFactionByFaction",
];
/// The routines a script ends a fight with: a round's end.
const ROUND_END: [&str; 4] = [
    "SurrenderToEnemies",
    "SurrenderRetainBuffs",
    "SurrenderByFaction",
    "CancelCombat",
];
/// The protections the gate reads that a script sets.
const PROTECT: [&str; 2] = ["SetMinOneHP", "SetPlotFlag"];

/// A module's own scripts by the routines of the three kinds each calls.
#[derive(Default)]
struct Module {
    hostile: BTreeSet<String>,
    ends: BTreeSet<String>,
    protects: BTreeSet<String>,
    /// Its own creature templates made Min1HP or plot.
    kept: BTreeSet<String>,
}

impl Module {
    /// A scripted round: someone turned hostile, a fight ended by script.
    fn round(&self) -> bool {
        !self.hostile.is_empty() && !self.ends.is_empty()
    }

    fn covered(&self) -> bool {
        !self.protects.is_empty() || !self.kept.is_empty()
    }
}

/// Each module's own scripts, read (the shared layers' are every module's
/// and no round's).
fn modules(game: &str, discovery: &Discovery) -> Result<BTreeMap<String, Module>, String> {
    let (_, index, base) = open(discovery, game)?;
    let catalog = catalog(&base)?;
    let shared: BTreeSet<String> = base.layers.iter().map(|layer| layer.name.clone()).collect();
    let mut found = BTreeMap::new();
    for name in Resolver::modules(&index) {
        let Ok(resolver) = base.with_module(&index, &name) else {
            continue;
        };
        let mut module = Module::default();
        for layer in resolver.layers.iter().filter(|layer| !shared.contains(&layer.name)) {
            for ((script, restype), location) in &layer.resources {
                if *restype == ResType::UTC {
                    let kept = location.read().ok().and_then(|bytes| Gff::parse(&bytes).ok()).is_some_and(|utc| {
                        ["Plot", "Min1HP"].iter().any(|flag| utc.root.int(flag).unwrap_or(0) != 0)
                    });
                    if kept {
                        module.kept.insert(script.clone());
                    }
                    continue;
                }
                if *restype != ncs_type() {
                    continue;
                }
                let Some(ncs) = location.read().ok().and_then(|bytes| Ncs::parse(&bytes).ok())
                else {
                    continue;
                };
                for instruction in &ncs.code {
                    let Op::Action { routine, .. } = instruction.op else {
                        continue;
                    };
                    let Some(called) = catalog.routine_by_id(routine) else {
                        continue;
                    };
                    let into = if HOSTILE.contains(&called.name.as_str()) {
                        &mut module.hostile
                    } else if ROUND_END.contains(&called.name.as_str()) {
                        &mut module.ends
                    } else if PROTECT.contains(&called.name.as_str()) {
                        &mut module.protects
                    } else {
                        continue;
                    };
                    into.insert(script.clone());
                }
            }
        }
        found.insert(name, module);
    }
    Ok(found)
}

/// See the module documentation.
pub fn census(discovery: &Discovery, game: &str) -> ExitCode {
    let found = match modules(game, discovery) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let rounds: Vec<(&String, &Module)> =
        found.iter().filter(|(_, module)| module.round()).collect();
    let listed = |scripts: &BTreeSet<String>| scripts.iter().cloned().collect::<Vec<_>>().join(", ");
    let mut rows = Vec::new();
    let mut unprotected = Vec::new();
    for (name, module) in &rounds {
        let covered = module.covered();
        outln!(
            "{name}: {} — turned hostile by {}; ended by {}; protected by scripts {}, templates {}",
            if covered { "covered" } else { "[verify]" },
            listed(&module.hostile),
            listed(&module.ends),
            listed(&module.protects),
            listed(&module.kept)
        );
        if !covered {
            unprotected.push((*name).clone());
        }
        rows.push(json!({
            "module": name,
            "covered": covered,
            "hostile": module.hostile,
            "ends": module.ends,
            "protects": module.protects,
            "templates": module.kept,
        }));
    }
    outln!(
        "{game}: {} modules read; {} scripted rounds, {} covered by a protection the gate reads, {} [verify]{}",
        found.len(),
        rounds.len(),
        rounds.len() - unprotected.len(),
        unprotected.len(),
        if unprotected.is_empty() {
            String::new()
        } else {
            format!(": {}", unprotected.join(", "))
        }
    );
    crate::collision_verbs::write_facts(json!({
        "game": game,
        "modules": found.len(),
        "rounds": rows,
        "verify": unprotected,
    }));
    if rounds.is_empty() {
        eprintln!("{game}: no scripted round found: the census reads nothing");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
