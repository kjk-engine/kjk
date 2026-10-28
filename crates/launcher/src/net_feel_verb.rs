//! `kjk net feel`: the net-feel report of a host's and a player's
//! `net.json` (written as a session runs, `net_feel`).

use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use crate::net_feel::*;

/// What each figure is, in the report's words, in its order.
const FIGURES: [(&str, &str); 17] = [
    ("rtt_ms", "round trip (ms)"),
    ("jitter_ms", "jitter (ms)"),
    ("loss_percent", "loss (%)"),
    ("kb_down_s", "down (KB/s)"),
    ("kb_up_s", "up (KB/s)"),
    ("interval_ticks", "snapshot interval (ticks)"),
    ("snapshots_s", "snapshots received (/s)"),
    ("slack_ms", "input slack (ms)"),
    ("lead_ms", "lead (ms)"),
    ("delay_ms", "view lag: one way + D (ms)"),
    ("target_slack_ms", "target slack (ms)"),
    ("d_ms", "remote delay D (ms)"),
    ("jitter_p95_ms", "snapshot spread p95 (ms)"),
    ("loss_margin_frames", "loss margin (frames)"),
    ("frame_counted_ms", "frame the margin counts (ms)"),
    ("skipped_s", "ticks skipped (/s)"),
    ("substituted_s", "commands substituted (/s)"),
];

/// Mean, p95 and max of `values`.
fn spread(values: &mut [f64]) -> Option<(f64, f64, f64)> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let p95 = values[((values.len() - 1) as f64 * 0.95).round() as usize];
    Some((mean, p95, values[values.len() - 1]))
}

fn read(path: &Path) -> Result<Value, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The net-feel report of a host's and a player's `net.json`.
pub fn report(host: &Value, player: &Value) -> String {
    // Columns: each track of each side.
    let mut columns: Vec<(String, BTreeMap<&str, Vec<f64>>)> = Vec::new();
    for (side, facts) in [("host", host), ("player", player)] {
        let mut tracks: BTreeMap<String, BTreeMap<&str, Vec<f64>>> = BTreeMap::new();
        for row in facts["feel"]["samples"].as_array().into_iter().flatten() {
            let track = row["track"].as_str().unwrap_or("net").to_string();
            let figures = tracks.entry(track).or_default();
            for (key, _) in &FIGURES {
                if let Some(value) = row[*key].as_f64() {
                    figures.entry(key).or_default().push(value);
                }
            }
        }
        for (track, figures) in tracks {
            let name = match (side, track.strip_prefix("net seat ")) {
                ("host", Some(seat)) => format!("host, of seat {seat}"),
                _ => side.to_string(),
            };
            columns.push((name, figures));
        }
    }
    let mut out = String::new();
    let _ = writeln!(out, "# Net feel\n");
    let _ = writeln!(
        out,
        "Each side's samples every {} ms; each cell is mean / p95 / max.\n",
        SAMPLE.as_millis()
    );
    let _ = writeln!(
        out,
        "| | {} |",
        columns
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let _ = writeln!(out, "|---|{}", "---|".repeat(columns.len()));
    for (key, label) in &FIGURES {
        let cells: Vec<String> = columns
            .iter_mut()
            .map(|(_, figures)| {
                figures
                    .get_mut(key)
                    .and_then(|values| spread(values))
                    .map_or("–".to_string(), |(mean, p95, max)| {
                        format!("{mean:.1} / {p95:.1} / {max:.1}")
                    })
            })
            .collect();
        if cells.iter().any(|cell| cell != "–") {
            let _ = writeln!(out, "| {label} | {} |", cells.join(" | "));
        }
    }

    // The run's counts.
    let number = |value: &Value| value.as_f64().map_or("–".to_string(), |n| format!("{n}"));
    let _ = writeln!(out, "\n## The run\n");
    let p = player;
    let _ = writeln!(
        out,
        "- player: {} snapshots rebuilt ({} keyframes, {} check failures); {} own-avatar predictions compared, {} differed {}; {} rewinds (mean depth {:.1} ticks), {} corrections; {} starvations; installs {} agreed, {} differed",
        number(&p["rebuilt"]),
        number(&p["keyframes"]),
        number(&p["check_failures"]),
        number(&p["compared"]),
        number(&p["mismatches"]),
        p["by_cause"],
        number(&p["rewinds"]),
        p["rewind_mean"].as_f64().unwrap_or(0.0),
        number(&p["corrections"]),
        number(&p["starvations"]),
        number(&p["installs_agreed"]),
        number(&p["installs_differed"]),
    );
    for row in host["players"].as_array().into_iter().flatten() {
        let _ = writeln!(
            out,
            "- host, of seat {}: {} commands, {} late, {} substituted; {} keyframes; {} interval changes; {} packets declared lost ({} acked late)",
            number(&row["seat"]),
            number(&row["commands"]),
            number(&row["late"]),
            number(&row["substituted"]),
            number(&row["keyframes"]),
            number(&row["interval_changes"]),
            number(&row["declared_lost"]),
            number(&row["late_acks"]),
        );
    }
    let _ = writeln!(
        out,
        "- the host resumed without a player still loading {} time(s)",
        number(&host["late_resumes"])
    );
    // When each player's snapshot interval changed (the adaptive rate).
    let mut changes: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut was: BTreeMap<&str, f64> = BTreeMap::new();
    for row in host["feel"]["samples"].as_array().into_iter().flatten() {
        let (Some(track), Some(every), Some(t)) = (
            row["track"].as_str(),
            row["interval_ticks"].as_f64(),
            row["t"].as_f64(),
        ) else {
            continue;
        };
        if let Some(before) = was.insert(track, every)
            && before != every
        {
            changes
                .entry(track)
                .or_default()
                .push(format!("{before} → {every} at {t:.1} s"));
        }
    }
    for (track, changes) in changes {
        let _ = writeln!(
            out,
            "- {track}'s snapshot interval (ticks): {}",
            changes.join(", ")
        );
    }

    // Input to photon.
    let mean_of = |facts: &Value, key: &str| -> f64 {
        let values: Vec<f64> = facts["feel"]["samples"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row[key].as_f64())
            .collect();
        values.iter().sum::<f64>() / values.len().max(1) as f64
    };
    let frame = |facts: &Value| facts["feel"]["frame_ms_mean"].as_f64().unwrap_or(0.0);
    let (lead, delay) = (mean_of(player, "lead_ms"), mean_of(player, "delay_ms"));
    let _ = writeln!(out, "\n## Input to photon (means)\n");
    let _ = writeln!(
        out,
        "- local (the player's input to its own predicted result on screen): one frame and at most a tick, {:.0} ms",
        frame(player) + TICK_MS
    );
    let _ = writeln!(
        out,
        "- to authority (the player's input applied by the host): its lead, {lead:.0} ms"
    );
    let _ = writeln!(
        out,
        "- remote, the host's input on the player's screen: the host's lead (0: it plays in lockstep) + the player's view lag + a frame, {:.0} ms",
        delay + frame(player)
    );
    let _ = writeln!(
        out,
        "- remote, the player's input on the host's screen: the player's lead + a frame, {:.0} ms",
        lead + frame(host)
    );
    out
}

/// `kjk net feel`: the net-feel report of a host's and a player's
/// `net.json`, to stdout.
pub fn feel_verb(host: &Path, player: &Path) -> ExitCode {
    match (read(host), read(player)) {
        (Ok(host), Ok(player)) => {
            print!("{}", report(&host, &player));
            ExitCode::SUCCESS
        }
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
