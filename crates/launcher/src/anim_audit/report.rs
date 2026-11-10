//! The audit's results: `cases.json` (each case, how to run it again, and
//! each state's worst values), `report.md` (the failing rows, worst first,
//! and every state's values), and the comparison with an earlier run
//! (`--against`).

use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::cases::{Case, Rules};
use super::checks::{Check, Worst};

/// The file of every case and its values, in the output folder.
pub const CASES: &str = "cases.json";
/// The report's file, in the output folder.
pub const REPORT: &str = "report.md";

/// A value rises by more than this share of its check's limit to count as
/// worse against an earlier run.
const WORSE_SHARE: f32 = 0.05;

/// One case of a run: what it is, the `kjk` arguments and environment that
/// run it again, why its run failed when it did, and its states.
#[derive(Debug, Serialize, Deserialize)]
pub struct Record {
    pub case: Case,
    pub name: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    /// Why the run failed (its exit, a missing log), with its error output's
    /// last lines. A failed case has no states.
    pub failed: Option<String>,
    pub states: Vec<StateRecord>,
}

/// One state of a case: how many frames it has and how many of them hold a
/// lit blade, whether the skeleton lacked its sequence so JA's played
/// (`fallback`), and each check's worst.
#[derive(Debug, Serialize, Deserialize)]
pub struct StateRecord {
    pub state: String,
    pub frames: usize,
    /// The frames with at least one blade lit and not growing.
    #[serde(default)]
    pub lit_frames: usize,
    /// The state has frames, but none with a lit blade, and it is not the
    /// holster (`cases::HOLSTER`). It has no worst values and fails.
    #[serde(default)]
    pub no_lit_blade: bool,
    pub fallback: bool,
    pub worst: Vec<WorstRecord>,
}

/// A check's worst value in a state, with its limit and moment (sim ms).
#[derive(Debug, Serialize, Deserialize)]
pub struct WorstRecord {
    pub check: String,
    /// A value that is not a finite number (a measure that broke) is kept
    /// as `null` and read back as NaN.
    #[serde(with = "not_finite_as_null")]
    pub value: f32,
    pub limit: f32,
    pub ms: f64,
}

impl From<Worst> for WorstRecord {
    fn from(worst: Worst) -> WorstRecord {
        WorstRecord {
            check: worst.check.name().to_string(),
            value: worst.value,
            limit: worst.check.limit(),
            ms: worst.ms,
        }
    }
}

/// A value written as a number when it is finite, else as `null`, which is
/// read back as NaN. JSON has no NaN, and serde_json writes one as `null`
/// that it cannot read back as a number.
mod not_finite_as_null {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
        if value.is_finite() {
            serializer.serialize_some(value)
        } else {
            serializer.serialize_none()
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
        Ok(Option::<f32>::deserialize(deserializer)?.unwrap_or(f32::NAN))
    }
}

impl WorstRecord {
    /// Over the limit, or not a number: a NaN fails.
    fn fails(&self) -> bool {
        self.value.is_nan() || self.value > self.limit
    }
}

/// The records of an earlier run's `cases.json` at `path`.
pub fn read(path: &Path) -> Result<Vec<Record>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// `cases.json` and `report.md` of `records`, into `out`.
pub fn write(out: &Path, records: &[Record]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(records).map_err(|error| error.to_string())?;
    let cases = out.join(CASES);
    std::fs::write(&cases, json).map_err(|error| format!("{}: {error}", cases.display()))?;
    let report = out.join(REPORT);
    std::fs::write(&report, markdown(records))
        .map_err(|error| format!("{}: {error}", report.display()))
}

/// A check's value as the report writes it: whole numbers for the checks
/// with large limits (pops, spin), else three decimals.
fn shown(value: f32, limit: f32) -> String {
    if limit >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.3}")
    }
}

/// The report's text. The failed runs come first. The failing rows follow,
/// worst first by how many times its limit each value is, then every
/// state's values. A state with no frame counts as a failing row.
fn markdown(records: &[Record]) -> String {
    let mut text = String::from("# Saber animation audit\n\n");
    let failed: Vec<&Record> = records
        .iter()
        .filter(|record| record.failed.is_some())
        .collect();
    let mut failing: Vec<(&Record, &StateRecord, &WorstRecord)> = records
        .iter()
        .flat_map(|record| {
            record.states.iter().flat_map(move |state| {
                state
                    .worst
                    .iter()
                    .filter(|worst| worst.fails())
                    .map(move |worst| (record, state, worst))
            })
        })
        .collect();
    failing.sort_by(|a, b| (b.2.value / b.2.limit).total_cmp(&(a.2.value / a.2.limit)));
    let unmeasured: Vec<(&Record, &StateRecord, &str)> = records
        .iter()
        .flat_map(|record| record.states.iter().map(move |state| (record, state)))
        .filter_map(|(record, state)| Some((record, state, unmeasured(state)?)))
        .collect();
    let _ = writeln!(
        text,
        "{} cases. {} runs failed. {} rows fail their check's limit, and {} states have no frame or no lit blade.\n",
        records.len(),
        failed.len(),
        failing.len(),
        unmeasured.len()
    );
    if !failed.is_empty() {
        text.push_str("## Failed runs\n\n");
        for record in &failed {
            let why = record.failed.as_deref().unwrap_or("");
            let _ = writeln!(text, "### {}\n\n```\n{why}\n```\n", record.name);
        }
    }
    text.push_str("## Failing rows\n\n");
    text.push_str("| Case | State | Check | Value | Limit | ms |\n|---|---|---|---|---|---|\n");
    for (record, state, why) in &unmeasured {
        let _ = writeln!(text, "| {} | {} | {why} | | | |", record.name, state.state);
    }
    for (record, state, worst) in &failing {
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {} | {:.0} |",
            record.name,
            state.state,
            worst.check,
            shown(worst.value, worst.limit),
            shown(worst.limit, worst.limit),
            worst.ms
        );
    }
    text.push_str(
        "\n## Every state\n\nA value over its check's limit is in bold. A dash is a check that does \
         not apply to the state. A fallback state is one in which the skeleton lacks the \
         sequence, so JA's own played.\n\n",
    );
    text.push_str("| Case | State | Frames | Lit | Fallback |");
    for check in Check::ALL {
        let _ = write!(text, " {} |", check.name());
    }
    text.push_str("\n|---|---|---|---|---|");
    text.push_str(&"---|".repeat(Check::ALL.len()));
    text.push('\n');
    for record in records {
        for state in &record.states {
            let fallback = if state.fallback { "yes" } else { "" };
            let _ = write!(
                text,
                "| {} | {} | {} | {} | {fallback} |",
                record.name, state.state, state.frames, state.lit_frames
            );
            for check in Check::ALL {
                let cell = match state.worst.iter().find(|w| w.check == check.name()) {
                    Some(worst) if worst.fails() => {
                        format!("**{}**", shown(worst.value, worst.limit))
                    }
                    Some(worst) => shown(worst.value, worst.limit),
                    None => "-".to_string(),
                };
                let _ = write!(text, " {cell} |");
            }
            text.push('\n');
        }
    }
    text
}

/// Why a state was not measured: it has no frame, or no lit blade.
fn unmeasured(state: &StateRecord) -> Option<&'static str> {
    if state.frames == 0 {
        Some("no frame")
    } else if state.no_lit_blade {
        Some("no lit blade")
    } else {
        None
    }
}

/// Whether two values are the same, NaN being the same as NaN.
fn same(a: f32, b: f32) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// What got worse in `new` against `old`, a line each, for the cases both
/// have. A case whose run failed now and passed before is worse, and so is
/// a state that had frames and has none, or had lit frames and has no lit
/// blade. A check's value is worse when it
/// passed and now fails, or when it rose by more than `WORSE_SHARE` of its
/// limit. Under JA's rules any change of a value at all counts.
pub fn worse(old: &[Record], new: &[Record]) -> Vec<String> {
    let mut lines = Vec::new();
    for record in new {
        let Some(before) = old.iter().find(|old| old.name == record.name) else {
            continue;
        };
        if let Some(why) = &record.failed {
            if before.failed.is_none() {
                let first = why.lines().next().unwrap_or("");
                lines.push(format!("{}: the run failed ({first})", record.name));
            }
            continue;
        }
        let ja = record.case.rules == Rules::Ja;
        for state in &record.states {
            let Some(was) = before.states.iter().find(|was| was.state == state.state) else {
                continue;
            };
            if was.frames > 0 && state.frames == 0 {
                lines.push(format!("{}, {}: no frame now", record.name, state.state));
                continue;
            }
            if was.lit_frames > 0 && state.no_lit_blade {
                lines.push(format!(
                    "{}, {}: no lit blade now",
                    record.name, state.state
                ));
                continue;
            }
            for worst in &state.worst {
                let Some(earlier) = was.worst.iter().find(|w| w.check == worst.check) else {
                    continue;
                };
                let row = format!(
                    "{}, {}, {}: {} to {} (limit {})",
                    record.name,
                    state.state,
                    worst.check,
                    shown(earlier.value, worst.limit),
                    shown(worst.value, worst.limit),
                    shown(worst.limit, worst.limit)
                );
                if ja && !same(worst.value, earlier.value) {
                    lines.push(format!("{row}, changed under JA's rules"));
                } else if !earlier.fails() && worst.fails() {
                    lines.push(format!("{row}, fails now"));
                } else if worst.value - earlier.value > WORSE_SHARE * worst.limit {
                    lines.push(format!("{row}, rose"));
                }
            }
        }
    }
    lines
}
