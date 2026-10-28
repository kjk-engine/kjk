//! Equivalence traces (`kjk trace`, `kjk trace-diff`; `docs/EQUIVALENCE.md`): a
//! package of the sim driven through its public API alone, one record per
//! step written as text with every float as its bits, so two builds of the
//! same package can be compared exactly.
//!
//! The drivers here call only what a package exports (`pub` or
//! `pub(crate)`), never its insides: the same driver compiles against any
//! code that keeps those signatures, which is the point. Each package's
//! list of what is called is its frozen API (`context/cleanroom/<pkg>/
//! API.md`).
//!
//! - `scenario`: the scenario files (named, hand-written or generated), a
//!   line of words per directive;
//! - `record`: one step's record, and the trace file;
//! - `geometry`: the test worlds the movement scenarios walk (boxes, ramps,
//!   stairs, ledges, ceilings, wedges; all made here);
//! - `movement`: `m1` (and `m3`): `move_body` with or without a fight;
//! - `moves`: `m2`: the saber move table and its queries;
//! - `rules`: `m4` and `m5`: the saber rules' and the blade AI's pure
//!   functions;
//! - `duels`: `m6`: the world's blades, players fighting in a `SimWorld`;
//! - `diff`: the first divergence between two traces.

mod diff;
mod duels;
mod geometry;
mod movement;
mod moves;
mod record;
mod rules;
mod scenario;

use std::path::{Path, PathBuf};

pub use diff::{Divergence, diff};
pub use scenario::Scenario;

use record::Trace;

/// The packages a trace drives, and what each covers.
pub const PACKAGES: &[(&str, &str)] = &[
    (
        "m1",
        "movement: move_body's step, slides, steps, jumps, force jumps",
    ),
    ("m2", "the saber move table and its queries"),
    (
        "m3",
        "move_body's weapon step: saber moves, weapons, animations",
    ),
    (
        "m4",
        "the saber rules: clashes, power, locks, defence, kicks, look",
    ),
    ("m5", "the blade wielder's pacing"),
    (
        "m6",
        "the world's blades: locks, clashes, kicks, throws, bolt blocks",
    ),
];

/// Where a trace's scenarios come from.
pub enum Source {
    /// Scenario files (a folder: every `.scn` in it, by name).
    Files(Vec<PathBuf>),
    /// The package's seeded generator.
    Random,
}

/// How many random scenarios, of how many steps, from which seed.
#[derive(Debug, Clone)]
pub struct Options {
    /// The first seed.
    pub seed: u64,
    /// Seeds in a row from `seed`.
    pub seeds: u64,
    /// Steps per seed.
    pub n: u64,
    /// Where to write the generated scenarios as a scenario file, so a
    /// divergence can be kept as a named scenario.
    pub emit: Option<PathBuf>,
}

/// What a run wrote.
#[derive(Debug, Clone, Copy, Default)]
pub struct Summary {
    pub scenarios: usize,
    pub steps: u64,
}

/// The scenarios `source` names for `pkg`.
pub fn scenarios(pkg: &str, source: &Source, options: &Options) -> Result<Vec<Scenario>, String> {
    match source {
        Source::Files(paths) => {
            let mut all = Vec::new();
            for path in expand(paths)? {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                all.extend(scenario::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?);
            }
            Ok(all)
        }
        Source::Random => {
            let mut all = Vec::new();
            for seed in options.seed..options.seed + options.seeds.max(1) {
                all.extend(match pkg {
                    "m1" => movement::random(seed, options.n, false),
                    "m3" => movement::random(seed, options.n, true),
                    "m2" => moves::random(seed, options.n),
                    "m4" => rules::random_m4(seed, options.n),
                    "m5" => rules::random_m5(seed, options.n),
                    "m6" => duels::random(seed, options.n),
                    _ => return Err(unknown(pkg)),
                });
            }
            Ok(all)
        }
    }
}

/// Runs `pkg`'s scenarios from `source` and writes their trace to `out`.
pub fn run(pkg: &str, source: &Source, options: &Options, out: &Path) -> Result<Summary, String> {
    if !PACKAGES.iter().any(|(name, _)| *name == pkg) {
        return Err(unknown(pkg));
    }
    let all = scenarios(pkg, source, options)?;
    if let Some(emit) = &options.emit {
        let text: String = all.iter().map(Scenario::to_text).collect();
        std::fs::write(emit, text).map_err(|e| format!("{}: {e}", emit.display()))?;
    }
    let mut trace = Trace::create(out, pkg)?;
    let mut summary = Summary::default();
    for scenario in &all {
        trace.begin(&scenario.name)?;
        let steps = match pkg {
            "m1" | "m3" => movement::run(scenario, &mut trace),
            "m2" => moves::run(scenario, &mut trace),
            "m4" | "m5" => rules::run(scenario, &mut trace),
            "m6" => duels::run(scenario, &mut trace),
            _ => Err(unknown(pkg)),
        }
        .map_err(|e| format!("scenario {}: {e}", scenario.name))?;
        summary.scenarios += 1;
        summary.steps += steps;
    }
    trace.finish()?;
    Ok(summary)
}

fn unknown(pkg: &str) -> String {
    let names: Vec<&str> = PACKAGES.iter().map(|(name, _)| *name).collect();
    format!("no package {pkg} (one of {})", names.join(", "))
}

/// Files as given; a folder's `.scn` files by name.
fn expand(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.extension().is_some_and(|ext| ext == "scn"))
                .collect();
            found.sort();
            out.extend(found);
        } else {
            out.push(path.clone());
        }
    }
    Ok(out)
}
