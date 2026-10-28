//! `kjk script sweep <game>`: every module of an install installed for one
//! player and stepped headless, its scripts running (`script_verbs::
//! step_module`), and what blocks the games' own scripts across the whole
//! game: the routines they call that are gaps, by calls and by how many
//! modules call them; the faults; the scripts the game names but does not
//! have (nothing runs: a gap to a player); the modules that do not
//! install. What to bind next is what the most of the game needs
//! (`ROADMAP.md`, principle 4: the scripts work out of the box, touch-ups
//! after).

use std::collections::BTreeMap;
use std::process::ExitCode;

use serde_json::json;
use vfs::{Discovery, Game};

use crate::script_verbs::{Coverage, Stepped, step_module};

/// One module's sweep.
#[derive(Default)]
pub struct Swept {
    pub runs: usize,
    pub instructions: usize,
    pub gaps: BTreeMap<String, usize>,
    /// Faults, and runs stopped at the instruction limit.
    pub faults: Vec<String>,
    /// Scripts named but not in the library, by times named.
    pub missing: BTreeMap<String, usize>,
    pub ms: f64,
}

/// One module stepped `options.seconds` with its scripts running; why not,
/// when it does not install or panics.
pub fn sweep_module(
    installer: &crate::lifecycle::Installer,
    module: &str,
    options: &Coverage,
) -> Result<Swept, String> {
    let stepped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        step_module(installer, module, options)
    }));
    match stepped {
        Ok(Ok(Stepped { world, ms, .. })) => {
            let report = world.script_report();
            Ok(Swept {
                runs: report.runs.values().sum::<usize>() + report.deferred.values().sum::<usize>(),
                instructions: report.instructions,
                gaps: report.gaps.clone(),
                faults: report.faults.iter().chain(&report.runaways).cloned().collect(),
                missing: report.missing.clone(),
                ms,
            })
        }
        Ok(Err(error)) => Err(format!("not installed: {error}")),
        Err(_) => Err("panicked".into()),
    }
}

/// The options a sweep steps each module with.
pub fn coverage(seconds: u64) -> Coverage {
    Coverage {
        seconds,
        trace: false,
        walk: None,
        frames: None,
        jedi: false,
    }
}

/// Gaps across modules, most reached first: (routine, (calls, modules)).
pub fn rank_gaps<'a>(swept: impl Iterator<Item = &'a Swept>) -> Vec<(&'a str, (usize, usize))> {
    rank(swept, |one| &one.gaps)
}

/// The names `counted` gives each module (its gaps, or its missing
/// scripts), across modules, most reached first: (name, (count, modules)).
pub fn rank<'a>(
    swept: impl Iterator<Item = &'a Swept>,
    counted: fn(&Swept) -> &BTreeMap<String, usize>,
) -> Vec<(&'a str, (usize, usize))> {
    let mut found: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for one in swept {
        for (name, count) in counted(one) {
            let entry = found.entry(name.as_str()).or_insert((0, 0));
            entry.0 += count;
            entry.1 += 1;
        }
    }
    let mut ranked: Vec<(&str, (usize, usize))> = found.into_iter().collect();
    ranked.sort_by(|a, b| b.1.1.cmp(&a.1.1).then(b.1.0.cmp(&a.1.0)).then(a.0.cmp(b.0)));
    ranked
}

/// `kjk script sweep`: `seconds` of each module (those starting with
/// `only`, if given).
pub fn sweep(discovery: &Discovery, game: &str, seconds: u64, only: Option<&str>) -> ExitCode {
    let run = || -> Result<serde_json::Value, String> {
        let game = Game::from_key(game).ok_or(format!("{game:?}: use k1 or tsl"))?;
        let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
        let installer = crate::lifecycle::Installer::new(discovery, game, anims)?;
        let options = coverage(seconds);
        let modules: Vec<String> = installer
            .modules()
            .into_iter()
            .filter(|module| only.is_none_or(|only| module.starts_with(only)))
            .collect();
        let started = std::time::Instant::now();
        let mut swept: BTreeMap<String, Swept> = BTreeMap::new();
        let mut failed: BTreeMap<String, String> = BTreeMap::new();
        for module in &modules {
            match sweep_module(&installer, module, &options) {
                Ok(one) => {
                    outln!(
                        "  {module:<12} {:>6} runs {:>3} gaps {:>3} missing {:>3} faults {:>6.0} ms",
                        one.runs,
                        one.gaps.len(),
                        one.missing.len(),
                        one.faults.len(),
                        one.ms
                    );
                    swept.insert(module.clone(), one);
                }
                Err(error) => {
                    outln!("  {module:<12} {error}");
                    failed.insert(module.clone(), error);
                }
            }
        }
        let ranked = rank_gaps(swept.values());
        let missing = rank(swept.values(), |one| &one.missing);
        let faulted: Vec<(&String, &Vec<String>)> = swept
            .iter()
            .filter(|(_, one)| !one.faults.is_empty())
            .map(|(module, one)| (module, &one.faults))
            .collect();
        let runs: usize = swept.values().map(|one| one.runs).sum();
        outln!(
            "{}: {} modules swept ({} s each), {} not installed; {runs} script runs; {} modules with faults ({:.0} s)",
            game.key(),
            swept.len(),
            seconds,
            failed.len(),
            faulted.len(),
            started.elapsed().as_secs_f64()
        );
        outln!("  gaps by modules reaching them (calls):");
        for (name, (calls, reached)) in ranked.iter().take(60) {
            outln!("    {name:<36} {reached:>4} modules {calls:>7} calls");
        }
        outln!("  scripts named but missing, nothing runs (modules, times named):");
        for (name, (named, reached)) in &missing {
            outln!("    {name:<36} {reached:>4} modules {named:>7} named");
        }
        for (module, faults) in &faulted {
            outln!("  faults in {module}: {}", faults.join("; "));
        }
        Ok(json!({
            "game": game.key(),
            "seconds": seconds,
            "modules": swept.iter().map(|(module, one)| (module.clone(), json!({
                "runs": one.runs,
                "instructions": one.instructions,
                "gaps": one.gaps,
                "faults": one.faults,
                "missing": one.missing,
                "ms": one.ms,
            }))).collect::<BTreeMap<_, _>>(),
            "not_installed": failed,
            "missing": missing.iter().map(|(name, (named, reached))| json!({
                "script": name, "named": named, "modules": reached,
            })).collect::<Vec<_>>(),
            "gaps": ranked.iter().map(|(name, (calls, reached))| json!({
                "routine": name, "calls": calls, "modules": reached,
            })).collect::<Vec<_>>(),
        }))
    };
    match run() {
        Ok(facts) => {
            crate::collision_verbs::write_facts(facts);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
