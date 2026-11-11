//! The audit's results: `cases.json` (each case, how to run it again, and
//! each state's worst values), `report.md` (the failing rows under MDA's
//! rules, the failing rows under JA's rules, and every state's values), the
//! clips of the worst failing MDA rows (`--clips`), and the comparison with
//! an earlier run (`--against`).

use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::cases::{Body, Case, Rules, Weapon};
use super::checks::{Check, Worst};
use super::clips;

/// The file of every case and its values, in the output folder.
pub const CASES: &str = "cases.json";
/// The report's file, in the output folder.
pub const REPORT: &str = "report.md";
/// The folder of the clips of the worst failing rows (`clips::film`), in
/// the output folder.
pub const CLIPS: &str = "clips";

/// A value rises by more than this share of its check's design limit to
/// count as worse against an earlier run.
const WORSE_SHARE: f32 = 0.05;

/// Under JA's rules a value counts as changed against an earlier run only
/// when it moves by more than this share of its check's design limit. Under
/// machine load a run's view eases a little differently in its first
/// seconds, which moves a value in its third decimal place although nothing
/// drawn changed.
const JA_NOISE_SHARE: f32 = 0.001;

/// Under MDA's rules a value fails only when it is more than this share of
/// its check's design limit over the JA-rules value of the same check,
/// weapon, body and state (`floor`).
const JA_MARGIN: f32 = 0.05;

/// Under MDA's rules a swing-start check is judged by this many times the
/// JA-rules value of the same weapon, body and state, in place of the margin
/// above (`floor`). The ratio is the one the design spec sets in its section
/// 2.4.
const SWING_OVER_JA: f32 = 1.5;

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

/// A check's worst value in a state, with the limit it is judged by and
/// its moment (sim ms).
#[derive(Debug, Serialize, Deserialize)]
pub struct WorstRecord {
    pub check: String,
    /// A value that is not a finite number (a measure that broke) is kept
    /// as `null` and read back as NaN.
    #[serde(with = "not_finite_as_null")]
    pub value: f32,
    /// The limit the value is judged by. It is the check's design limit
    /// (`Check::limit`), or the JA floor where that is higher (`floor`).
    pub limit: f32,
    /// The JA-rules value of the same check, weapon, body and state, kept
    /// on an MDA row when the run has one (`floor`).
    #[serde(default)]
    pub ja: Option<f32>,
    pub ms: f64,
}

impl From<Worst> for WorstRecord {
    fn from(worst: Worst) -> WorstRecord {
        WorstRecord {
            check: worst.check.name().to_string(),
            value: worst.value,
            limit: worst.check.limit(),
            ja: None,
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

    /// The check's design limit (`Check::limit`). A check this build does
    /// not know keeps the limit it was written with.
    pub fn design(&self) -> f32 {
        Check::named(&self.check).map_or(self.limit, Check::limit)
    }
}

/// Sets the limit each row of `records` is judged by. JA's rules give
/// kjk's shipped look, so under MDA's rules a value no worse than theirs is
/// not a fault. An MDA row whose case has a JA-rules value of the same
/// check, weapon, body and state among `records` keeps that value. It is
/// judged by the higher of its check's design limit and that value plus
/// `JA_MARGIN` of the design limit. The two swing-start checks are the
/// exception. An MDA row of one of them that has a JA-rules value is judged
/// by `SWING_OVER_JA` times that value, whether that is higher or lower than
/// the design limit. Every other row is judged by the design
/// limit alone, and so are the JA-rules rows themselves. A JA-rules value
/// that is not a number gives no floor. A run has one JA-rules case for
/// each weapon and body (`cases::cases`), so each row has at most one JA
/// value.
pub fn floor(records: &mut [Record]) {
    // A row's weapon, body, state and check.
    let key = |case: &Case, state: &str, check: &str| -> (Weapon, Body, String, String) {
        (case.weapon, case.body, state.to_string(), check.to_string())
    };
    let mut ja = std::collections::HashMap::new();
    for record in records
        .iter()
        .filter(|record| record.case.rules == Rules::Ja)
    {
        for state in &record.states {
            for worst in &state.worst {
                let row = key(&record.case, &state.state, &worst.check);
                let twice = ja.insert(row, worst.value).is_some();
                debug_assert!(!twice, "two JA-rules cases of {}", record.name);
            }
        }
    }
    for record in records.iter_mut() {
        let case = &record.case;
        for state in &mut record.states {
            for worst in &mut state.worst {
                let design = worst.design();
                let found = ja.get(&key(case, &state.state, &worst.check)).copied();
                worst.ja = found.filter(|value| case.rules == Rules::Mda && value.is_finite());
                let swing = matches!(
                    Check::named(&worst.check),
                    Some(Check::SwingStartTurn | Check::SwingStartTravel)
                );
                worst.limit = match worst.ja {
                    Some(value) if swing => SWING_OVER_JA * value,
                    Some(value) => design.max(value + JA_MARGIN * design),
                    None => design,
                };
            }
        }
    }
}

/// The records of an earlier run's `cases.json` at `path`.
pub fn read(path: &Path) -> Result<Vec<Record>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The records of an earlier run's `cases.json` at `path`, judged by
/// today's design limits and JA floor (`floor`) rather than by the limits
/// the file was written with. A row that passed then by a lower limit is
/// then seen to fail now.
pub fn earlier(path: &Path) -> Result<Vec<Record>, String> {
    let mut records = read(path)?;
    floor(&mut records);
    Ok(records)
}

/// `cases.json` and `report.md` of `records`, into `out`.
pub fn write(out: &Path, records: &[Record]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(records).map_err(|error| error.to_string())?;
    let cases = out.join(CASES);
    std::fs::write(&cases, json).map_err(|error| format!("{}: {error}", cases.display()))?;
    let report = out.join(REPORT);
    std::fs::write(&report, markdown(records, &out.join(CLIPS)))
        .map_err(|error| format!("{}: {error}", report.display()))
}

/// One failing row: a check's worst value in a state of a case, over its
/// limit or not a number.
pub struct Row<'a> {
    pub record: &'a Record,
    pub state: &'a StateRecord,
    pub worst: &'a WorstRecord,
}

impl Row<'_> {
    /// The file name of the row's clip, when the row is `rank`th of the
    /// failing rows (from 0). The rank comes first, so the clips sort worst
    /// first.
    pub fn clip(&self, rank: usize) -> String {
        format!(
            "{:02}-{}-{}-{}.gif",
            rank + 1,
            self.record.name.replace(' ', "-"),
            self.state.state,
            self.worst.check
        )
    }
}

/// The failing rows of the records whose case plays under `rules`, worst
/// first by how many times its limit each value is. A NaN counts as the
/// worst of all.
pub fn failing(records: &[Record], rules: Rules) -> Vec<Row<'_>> {
    let mut rows: Vec<Row> = records
        .iter()
        .filter(|record| record.case.rules == rules)
        .flat_map(|record| {
            record.states.iter().flat_map(move |state| {
                state
                    .worst
                    .iter()
                    .filter(|worst| worst.fails())
                    .map(move |worst| Row {
                        record,
                        state,
                        worst,
                    })
            })
        })
        .collect();
    rows.sort_by(|a, b| times_limit(b.worst).total_cmp(&times_limit(a.worst)));
    rows
}

/// How many times its limit a worst value is. A value that is not a
/// number is infinitely many times it, so it sorts first.
fn times_limit(worst: &WorstRecord) -> f32 {
    let times = worst.value / worst.limit;
    if times.is_nan() { f32::INFINITY } else { times }
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

/// The report's text. The failed runs come first. The failing rows under
/// MDA's rules follow, worst first (`failing`), then the failing rows under
/// JA's rules, then every state's values. A state with no frame or no lit
/// blade counts as a failing row of its case's rules. A failing MDA row
/// whose clip is in `clips_dir` links it.
fn markdown(records: &[Record], clips_dir: &Path) -> String {
    let mut text = String::from("# Saber animation audit\n\n");
    let failed: Vec<&Record> = records
        .iter()
        .filter(|record| record.failed.is_some())
        .collect();
    let mda = failing(records, Rules::Mda);
    let ja = failing(records, Rules::Ja);
    let unmeasured = |rules| -> Vec<(&Record, &StateRecord, &str)> {
        records
            .iter()
            .filter(|record| record.case.rules == rules)
            .flat_map(|record| record.states.iter().map(move |state| (record, state)))
            .filter_map(|(record, state)| Some((record, state, unmeasured(state)?)))
            .collect()
    };
    let (mda_unmeasured, ja_unmeasured) = (unmeasured(Rules::Mda), unmeasured(Rules::Ja));
    let _ = writeln!(
        text,
        "{} cases. {} runs failed. Under MDA's rules, {} rows fail their limit and {} states \
         have no frame or no lit blade. Under JA's rules, {} rows fail their limit and {} states \
         have no frame or no lit blade.\n",
        records.len(),
        failed.len(),
        mda.len(),
        mda_unmeasured.len(),
        ja.len(),
        ja_unmeasured.len()
    );
    if !failed.is_empty() {
        text.push_str("## Failed runs\n\n");
        for record in &failed {
            let why = record.failed.as_deref().unwrap_or("");
            let _ = writeln!(text, "### {}\n\n```\n{why}\n```\n", record.name);
        }
    }
    text.push_str("## Failing rows under MDA's rules\n\n");
    let _ = writeln!(
        text,
        "A row's limit is its check's design limit. A row whose case has a JA-rules value of the \
         same check, weapon, body and state fails only when it is also over that JA value plus \
         {:.0}% of the design limit. Its limit is then the higher of the two, and the row shows \
         the JA value. The two swing-start checks are the exception. Their limit is {} times the \
         JA value when the row has one.\n",
        JA_MARGIN * 100.0,
        SWING_OVER_JA
    );
    if clips_dir.is_dir() {
        let _ = writeln!(
            text,
            "A row's clip shows each audited frame from {} s before the row's moment to {} s \
             after it, from the duelist's side. It plays at quarter speed, so each 25 ms frame of \
             the game shows for 100 ms. The drawn blade is in its own colour, the sim's blade \
             is green, JA's blade is cyan, and a yellow line joins each to the drawn tip. Only \
             the rows under MDA's rules have clips.\n",
            clips::BEFORE_MS as f64 / 1000.0,
            clips::AFTER_MS as f64 / 1000.0
        );
    }
    rows_table(&mut text, &mda_unmeasured, &mda, clips_dir);
    text.push_str(
        "\n## Failing rows under JA's rules\n\nThese rows are kjk's shipped JA look. They are \
         judged by the design limit alone, and they cannot be fixed without changing what JA's \
         rules draw. The freeze compares them from one run to the next (`--against`).\n\n",
    );
    rows_table(&mut text, &ja_unmeasured, &ja, Path::new(""));
    text.push_str(
        "\n## Every state\n\nA value over its row's limit is in bold. A dash is a check that does \
         not apply to the state. A fallback state is one in which the skeleton lacks the \
         sequence, so JA's own played. Only the states the stance swap covers can be \
         fallbacks. These are `stand`, `walk` and the `block_` states.\n\n",
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
                        format!("**{}**", shown(worst.value, worst.design()))
                    }
                    Some(worst) => shown(worst.value, worst.design()),
                    None => "-".to_string(),
                };
                let _ = write!(text, " {cell} |");
            }
            text.push('\n');
        }
    }
    text
}

/// A table of failing rows: first the states with no frame or no lit blade
/// (`unmeasured`), then `failing`. A row whose clip is in `clips_dir`
/// links it.
fn rows_table(
    text: &mut String,
    unmeasured: &[(&Record, &StateRecord, &str)],
    failing: &[Row],
    clips_dir: &Path,
) {
    text.push_str(
        "| Case | State | Check | Value | Limit | Design limit | JA value | ms | Clip |\n\
         |---|---|---|---|---|---|---|---|---|\n",
    );
    for (record, state, why) in unmeasured {
        let _ = writeln!(
            text,
            "| {} | {} | {why} | | | | | | |",
            record.name, state.state
        );
    }
    for (rank, row) in failing.iter().enumerate() {
        let clip = row.clip(rank);
        let link = if clips_dir.join(&clip).is_file() {
            format!("[gif]({CLIPS}/{clip})")
        } else {
            String::new()
        };
        let worst = row.worst;
        let design = worst.design();
        let ja = worst.ja.map_or(String::new(), |ja| shown(ja, design));
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {} | {} | {ja} | {:.0} | {link} |",
            row.record.name,
            row.state.state,
            worst.check,
            shown(worst.value, design),
            shown(worst.limit, design),
            shown(design, design),
            worst.ms
        );
    }
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
/// have. A case whose run failed now and passed before is worse. So is a
/// state that had frames and has none, and a state that had lit frames and
/// has no lit blade. A state that now falls back to JA's sequence, and did
/// not before, is worse, because the Movie Duels pose is lost. So is a
/// check that the earlier run had for a state and the new run lacks. A
/// check's value is worse when it passed and now fails, or when it rose by
/// more than `WORSE_SHARE` of its limit. Under JA's rules any change of a
/// value counts, if it moves by more than `JA_NOISE_SHARE` of its limit.
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
            if state.fallback && !was.fallback {
                lines.push(format!(
                    "{}, {}: falls back to JA's sequence now",
                    record.name, state.state
                ));
            }
            for earlier in &was.worst {
                if !state.worst.iter().any(|w| w.check == earlier.check) {
                    lines.push(format!(
                        "{}, {}, {}: no value now",
                        record.name, state.state, earlier.check
                    ));
                }
            }
            for worst in &state.worst {
                let Some(earlier) = was.worst.iter().find(|w| w.check == worst.check) else {
                    continue;
                };
                let design = worst.design();
                let row = format!(
                    "{}, {}, {}: {} to {} (limit {})",
                    record.name,
                    state.state,
                    worst.check,
                    shown(earlier.value, design),
                    shown(worst.value, design),
                    shown(worst.limit, design)
                );
                // A NaN on one side only makes the difference NaN, which
                // counts as moved.
                let step = (worst.value - earlier.value).abs();
                let noise = step.is_finite() && step <= JA_NOISE_SHARE * design;
                let moved = !same(worst.value, earlier.value) && !noise;
                if ja && moved {
                    lines.push(format!("{row}, changed under JA's rules"));
                } else if !earlier.fails() && worst.fails() {
                    lines.push(format!("{row}, fails now"));
                } else if worst.value - earlier.value > WORSE_SHARE * design {
                    lines.push(format!("{row}, rose"));
                }
            }
        }
    }
    lines
}
