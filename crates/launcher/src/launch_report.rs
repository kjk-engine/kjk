//! `kjk launch-report`: launches `kjk play` headless a few times, each
//! dumping where its time went (`launch_times`), and tables the phases and
//! moments (median, fastest, slowest), so a launch's cost is measured the
//! same way each time instead of with timing code added by hand.

use std::collections::BTreeMap;
use std::process::{Command, ExitCode};

use serde_json::{Value, json};

use crate::scenes::Report;

/// The console script each launch runs: up to the first frame of play, a
/// few frames on (the warm-up finishes), the dump.
const SCRIPT: &str = "wait world; wait 10f; dump launch; quit";

/// One launch's phases and moments, in the order they came.
type Launch = (Vec<(String, f64)>, Vec<(String, f64)>);

fn launch(stage: &str) -> Result<Launch, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(exe)
        .args(["play", "--stage", stage, "--headless", "--cmds", SCRIPT])
        .output()
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    let folder = text
        .lines()
        .find_map(|line| line.strip_prefix("run folder: "))
        .ok_or("the launch named no run folder")?
        .trim();
    let path = std::path::Path::new(folder).join("launch.json");
    let dump: Value = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or(format!(
            "{}: not written (exit {})",
            path.display(),
            output.status
        ))?;
    let list = |key: &str| -> Vec<(String, f64)> {
        dump["facts"]["launch"][key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| Some((item["name"].as_str()?.to_string(), item["ms"].as_f64()?)))
            .collect()
    };
    Ok((list("phases"), list("marks")))
}

/// (median, fastest, slowest) of `values`.
fn spread(values: &mut [f64]) -> (f64, f64, f64) {
    values.sort_by(f64::total_cmp);
    let median = values[values.len() / 2];
    (median, values[0], values[values.len() - 1])
}

/// Each name's values across launches, in the order they came (a phase
/// that runs twice, its times added), each phase above those inside it
/// (`outer/inner`), which finish, and so come, first.
fn gather(launches: &[Vec<(String, f64)>]) -> Vec<(String, Vec<f64>)> {
    let mut order: Vec<String> = Vec::new();
    let mut values: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for items in launches {
        let mut summed: BTreeMap<&str, f64> = BTreeMap::new();
        for (name, ms) in items {
            if !order.contains(name) {
                order.push(name.clone());
            }
            *summed.entry(name).or_insert(0.0) += ms;
        }
        for (name, ms) in summed {
            values.entry(name.to_string()).or_default().push(ms);
        }
    }
    // Where each of a name's enclosing phases came, outermost first, then
    // where it came itself.
    let place = |name: &str| -> Vec<usize> {
        let mut at: Vec<usize> = name
            .match_indices('/')
            .filter_map(|(end, _)| order.iter().position(|other| *other == name[..end]))
            .collect();
        at.extend(order.iter().position(|other| other == name));
        at
    };
    let mut sorted = order.clone();
    sorted.sort_by_key(|name| place(name));
    sorted
        .into_iter()
        .map(|name| {
            let found = values.remove(&name).unwrap_or_else(Vec::new);
            (name, found)
        })
        .collect()
}

fn table(title: &str, rows: &[(String, Vec<f64>)], runs: usize) -> Vec<Value> {
    outln!(
        "  {title:<46} {:>8} {:>8} {:>8}",
        "median",
        "fastest",
        "slowest"
    );
    rows.iter()
        .map(|(name, values)| {
            let (median, fastest, slowest) = spread(&mut values.clone());
            // A phase inside another is indented under it.
            let depth = name.matches('/').count();
            let label = format!(
                "{}{}",
                "  ".repeat(depth),
                name.rsplit('/').next().unwrap_or(name)
            );
            let partial = if values.len() < runs {
                format!(" ({} of {runs} runs)", values.len())
            } else {
                String::new()
            };
            outln!("  {label:<46} {median:>8.0} {fastest:>8.0} {slowest:>8.0}{partial}");
            json!({ "name": name, "median": median, "fastest": fastest, "slowest": slowest })
        })
        .collect()
}

pub fn launch_report(stage: &str, runs: usize) -> ExitCode {
    let mut report = Report::default();
    let runs = runs.max(1);
    let mut launches = Vec::new();
    for run in 1..=runs {
        match launch(stage) {
            Ok(launched) => launches.push(launched),
            Err(error) => outln!("  launch {run}: {error}"),
        }
    }
    report.check(
        launches.len() == runs,
        format!(
            "{} of {runs} launches of {stage} dumped their times",
            launches.len()
        ),
    );
    if launches.is_empty() {
        return report.finish(json!({ "stage": stage }));
    }
    let (phases, marks): (Vec<_>, Vec<_>) = launches.into_iter().unzip();
    outln!("{stage}, {runs} launches (ms):");
    let phases = table("phase", &gather(&phases), runs);
    outln!();
    let marks = table("since the process began", &gather(&marks), runs);
    report.finish(json!({ "stage": stage, "runs": runs, "phases": phases, "marks": marks }))
}
