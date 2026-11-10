//! The audit's results: `cases.json` (each case, how to run it again, and
//! each state's worst values), `report.md` (the failing rows, worst first,
//! and every state's values), the clips of the worst failing rows
//! (`--clips`), and the comparison with an earlier run (`--against`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::cases::{self, Case, Rules};
use super::checks::{Check, Worst};

/// The file of every case and its values, in the output folder.
pub const CASES: &str = "cases.json";
/// The report's file, in the output folder.
pub const REPORT: &str = "report.md";
/// The folder of the clips of the worst failing rows (`film`), in the
/// output folder.
pub const CLIPS: &str = "clips";

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

/// The failing rows of `records`, worst first by how many times its limit
/// each value is. A NaN counts as the worst of all.
pub fn failing(records: &[Record]) -> Vec<Row<'_>> {
    let mut rows: Vec<Row> = records
        .iter()
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

/// `film` films this many of the worst failing rows.
const FILMED: usize = 20;
/// A clip starts this long before its row's moment (ms), rounded down to a
/// step of the case's run.
const BEFORE_MS: u64 = 500;
/// A clip ends this long after its row's moment (ms).
const AFTER_MS: u64 = 1000;
/// The frames a clip records in each second of game time.
const RECORD_FPS: u32 = 60;
/// The gif's frames each second. Against `RECORD_FPS`, the gif plays the
/// game 6 times slower.
const GIF_FPS: u32 = 10;
/// The gif's width in pixels. Its height keeps the frames' shape.
const GIF_WIDTH: u32 = 640;
/// The name a clip's `record` commands give its frames, before their part
/// and number (`clip-a-0000.png`).
const FRAME_NAME: &str = "clip";

/// Gifs of the `FILMED` worst failing rows of `records`, into `out/clips`.
/// Each row's case is run again with the sim's and JA's blades drawn beside
/// the drawn one (`--sim-blades`) and the camera beside the duelist
/// (`--camera side`), and records from `BEFORE_MS` before the row's moment
/// to `AFTER_MS` after it. A clip that fails is told and left out. The clips
/// of an earlier run into `out` are removed first, so that the report links
/// none that shows another row.
pub fn film(exe: &Path, out: &Path, records: &[Record]) -> Result<(), String> {
    let clips = out.join(CLIPS);
    let _ = std::fs::remove_dir_all(&clips);
    std::fs::create_dir_all(&clips).map_err(|error| format!("{}: {error}", clips.display()))?;
    let rows: Vec<(String, Row)> = failing(records)
        .into_iter()
        .enumerate()
        .take(FILMED)
        .map(|(rank, row)| (row.clip(rank), row))
        .collect();
    eprintln!("anim audit: filming {} clips", rows.len());
    let made = cases::at_once(&rows, |(clip, row)| {
        let made = film_row(exe, out, row, clip);
        match &made {
            Ok(()) => eprintln!("anim audit: clip {clip}"),
            Err(why) => eprintln!("anim audit: clip {clip} failed: {why}"),
        }
        made.is_ok()
    });
    eprintln!(
        "anim audit: {} of {} clips made in {}",
        made.iter().filter(|made| **made).count(),
        rows.len(),
        clips.display()
    );
    Ok(())
}

/// The gif `clip` of `row`, into `out/clips`. The run's own files are in
/// `out/runs/clip-<name>`. Its frames are taken out of the run's folder in
/// order, made into the gif by `ffmpeg`, and removed.
fn film_row(exe: &Path, out: &Path, row: &Row, clip: &str) -> Result<(), String> {
    let io = |what: &Path, error: std::io::Error| format!("{}: {error}", what.display());
    let stem = clip.strip_suffix(".gif").unwrap_or(clip);
    let dir = out.join("runs").join(format!("clip-{stem}"));
    std::fs::create_dir_all(&dir).map_err(|error| io(&dir, error))?;
    let moment = row.worst.ms.max(0.0) as u64;
    let from = cases::step_floor(moment.saturating_sub(BEFORE_MS));
    let to = moment + AFTER_MS;
    let mut cmds = cases::steps(from);
    cmds.extend(recording(from, to));
    cmds.push("quit".to_string());
    let mut args = with_value(&row.record.args, "--cmds", cmds.join(";"));
    args = with_value(
        &args,
        "--audit-log",
        dir.join("frames.jsonl").display().to_string(),
    );
    args.extend(["--sim-blades", "--camera", "side"].map(String::from));
    let stderr = dir.join("stderr.log");
    cases::run_kjk(exe, &args, &row.record.env, &stderr)?;
    let log = std::fs::read_to_string(&stderr).map_err(|error| io(&stderr, error))?;
    let run = crate::tour::run_folder(&log).ok_or("the run named no run folder")?;
    let mut shots: Vec<PathBuf> = std::fs::read_dir(&run)
        .map_err(|error| io(&run, error))?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!("{FRAME_NAME}-")) && name.ends_with(".png")
                })
        })
        .collect();
    // The parts sort in their order (`clip-a-…` before `clip-b-…`), and
    // their frames by number.
    shots.sort();
    if shots.is_empty() {
        return Err(format!("{} has no frame of the clip", run.display()));
    }
    let frames = dir.join("frames");
    let _ = std::fs::remove_dir_all(&frames);
    std::fs::create_dir_all(&frames).map_err(|error| io(&frames, error))?;
    for (number, shot) in shots.iter().enumerate() {
        let frame = frames.join(format!("frame-{number:04}.png"));
        // The run's folder may be on another disk, where a rename fails.
        if std::fs::rename(shot, &frame).is_err() {
            std::fs::copy(shot, &frame).map_err(|error| io(shot, error))?;
        }
    }
    let made = gif(&frames, &out.join(CLIPS).join(clip));
    let _ = std::fs::remove_dir_all(&frames);
    made
}

/// The `--cmds` that record from `from` to `to` (ms of the run) at
/// `RECORD_FPS`. The case's run holsters the blade once the world has
/// reached `cases::holster_ms`. A recording across that moment does the
/// same. It records up to the moment, steps to it and holsters, then
/// records on as a second part.
fn recording(from: u64, to: u64) -> Vec<String> {
    let record = |part: &str, from: u64, to: u64| {
        format!(
            "record {FRAME_NAME}-{part} {} {} {RECORD_FPS}",
            from as f64 / 1000.0,
            to as f64 / 1000.0
        )
    };
    match cases::holster_ms() {
        Some(holster) if from < holster && holster < to => {
            let mut cmds = vec![record("a", from, holster)];
            cmds.extend(cases::step(holster));
            cmds.push("ui holster".to_string());
            cmds.push(record("b", holster, to));
            cmds
        }
        _ => vec![record("a", from, to)],
    }
}

/// `args` with the value after `flag` set to `value`, or with both added at
/// the end when `flag` is not among them.
fn with_value(args: &[String], flag: &str, value: String) -> Vec<String> {
    let mut args = args.to_vec();
    match args.iter().position(|arg| arg == flag) {
        Some(at) if at + 1 < args.len() => args[at + 1] = value,
        _ => args.extend([flag.to_string(), value]),
    }
    args
}

/// The gif `gif` of the numbered frames in `frames` (`frame-0000.png`), at
/// `GIF_FPS` and `GIF_WIDTH`, with a palette made from the frames.
fn gif(frames: &Path, gif: &Path) -> Result<(), String> {
    let filter =
        format!("scale={GIF_WIDTH}:-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse");
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-framerate",
            &GIF_FPS.to_string(),
        ])
        .arg("-i")
        .arg(frames.join("frame-%04d.png"))
        .args(["-vf", &filter, "-loop", "0"])
        .arg(gif)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("ffmpeg: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "ffmpeg: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
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
/// worst first (`failing`), then every state's values. A state with no
/// frame counts as a failing row. A failing row whose clip is in `clips`
/// links it.
fn markdown(records: &[Record], clips: &Path) -> String {
    let mut text = String::from("# Saber animation audit\n\n");
    let failed: Vec<&Record> = records
        .iter()
        .filter(|record| record.failed.is_some())
        .collect();
    let failing = failing(records);
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
    text.push_str(
        "| Case | State | Check | Value | Limit | ms | Clip |\n|---|---|---|---|---|---|---|\n",
    );
    for (record, state, why) in &unmeasured {
        let _ = writeln!(
            text,
            "| {} | {} | {why} | | | | |",
            record.name, state.state
        );
    }
    for (rank, row) in failing.iter().enumerate() {
        let clip = row.clip(rank);
        let link = if clips.join(&clip).is_file() {
            format!("[gif]({CLIPS}/{clip})")
        } else {
            String::new()
        };
        let worst = row.worst;
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {} | {:.0} | {link} |",
            row.record.name,
            row.state.state,
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
