//! `kjk report show|replay`: a player's bug report (`kjk_game::report`,
//! docs/BUG-REPORTS.md) opened here: what it says, and its last flight
//! recorder segment replayed on this machine's build and data.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sim::Recording;

use crate::levels::Installers;

/// How many of the crash's backtrace lines `show` prints.
const BACKTRACE_LINES: usize = 15;
/// How many of the log's last lines `show` prints.
const LOG_LINES: usize = 40;
/// How much a report may unpack to: far more than F12's 25 MB zips hold,
/// far less than a zip made to fill the disk.
const UNPACKED_LIMIT: u64 = 1 << 30;

/// `kjk report show`: what the report says.
pub(crate) fn show(zip: &Path) -> ExitCode {
    match Report::open(zip).and_then(|report| show_text(&report, &crate::discover())) {
        Ok(text) => {
            outln!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// `kjk report replay`: the report's last segment replayed headless;
/// without `headless`, how to watch it too.
pub(crate) fn replay(
    zip: &Path,
    to_tick: Option<u64>,
    headless: bool,
    ignore_data: bool,
) -> ExitCode {
    match replay_text(&crate::discover(), zip, to_tick, ignore_data) {
        Ok((text, matched, segment)) => {
            outln!("{text}");
            if !headless {
                outln!("watch it with: kjk sim view --replay {}", segment.display());
            }
            if matched {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// A report's files by name.
pub(crate) struct Report(BTreeMap<String, Vec<u8>>);

impl Report {
    /// The report at `zip`, its files read; a name with a folder in it is
    /// none a report writes, and is left out.
    pub(crate) fn open(zip: &Path) -> Result<Report, String> {
        let failed = |error: &dyn std::fmt::Display| format!("{}: {error}", zip.display());
        let file = std::fs::File::open(zip).map_err(|error| failed(&error))?;
        let mut archive = ::zip::ZipArchive::new(file).map_err(|error| failed(&error))?;
        let mut files = BTreeMap::new();
        let mut left = UNPACKED_LIMIT;
        for at in 0..archive.len() {
            let entry = archive.by_index(at).map_err(|error| failed(&error))?;
            let name = entry.name().map_err(|error| failed(&error))?.into_owned();
            if name.contains(['/', '\\']) || name.starts_with('.') || name.is_empty() {
                continue;
            }
            let mut bytes = Vec::new();
            entry
                .take(left + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| failed(&format!("{name}: {error}")))?;
            left = left
                .checked_sub(bytes.len() as u64)
                .ok_or_else(|| failed(&format!("unpacks to over {UNPACKED_LIMIT} bytes")))?;
            files.insert(name, bytes);
        }
        Ok(Report(files))
    }

    fn text(&self, name: &str) -> Option<String> {
        self.0
            .get(name)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    /// The flight recorder segments, oldest first.
    fn segments(&self) -> Vec<(&str, &[u8])> {
        let mut segments: Vec<(u32, &str, &[u8])> = self
            .0
            .iter()
            .filter_map(|(name, bytes)| {
                Some((
                    kjk_game::report::segment(name)?,
                    name.as_str(),
                    bytes.as_slice(),
                ))
            })
            .collect();
        segments.sort_by_key(|(number, ..)| *number);
        segments
            .into_iter()
            .map(|(_, name, bytes)| (name, bytes))
            .collect()
    }
}

/// This build's line (`kjk <release> (<commit>)`), as `system.txt` starts.
fn this_build() -> String {
    kjk_game::crash::facts()
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

/// `text`'s lines indented under a heading, or `(none)`.
fn section(out: &mut String, heading: &str, text: Option<&str>) {
    let _ = writeln!(out, "{heading}:");
    match text.map(str::trim_end).filter(|text| !text.is_empty()) {
        Some(text) => text.lines().for_each(|line| {
            let _ = writeln!(out, "  {line}");
        }),
        None => out.push_str("  (none)\n"),
    }
}

fn show_text(report: &Report, discovery: &vfs::Discovery) -> Result<String, String> {
    let mut out = String::new();
    let system = report.text("system.txt");
    let build = system
        .as_deref()
        .and_then(|system| system.lines().next())
        .unwrap_or("unknown")
        .to_string();
    let here = this_build();
    let _ = writeln!(
        out,
        "build: {build} ({})",
        if build == here {
            "same build as this one".to_string()
        } else {
            format!("this one is {here}")
        }
    );
    section(&mut out, "system", system.as_deref());
    section(&mut out, "note", report.text("note.txt").as_deref());
    section(
        &mut out,
        "breadcrumbs",
        report.text("breadcrumbs.txt").as_deref(),
    );
    section(
        &mut out,
        "crash",
        report
            .text("crash.txt")
            .map(|crash| crash_top(&crash))
            .as_deref(),
    );
    section(
        &mut out,
        &format!("log (its last {LOG_LINES} lines)"),
        report
            .text("kjk.log")
            .map(|log| last_lines(&log, LOG_LINES))
            .as_deref(),
    );
    let segments: Vec<String> = report
        .segments()
        .into_iter()
        .map(|(name, bytes)| match sim::flight::read(bytes) {
            Ok(recording) => {
                let (first, last) = span(&recording);
                format!(
                    "{name}: {} ticks {first}..{last} ({:.1} s), build {}",
                    recording.level.name,
                    seconds(&recording),
                    sim::flight::build(bytes).unwrap_or_else(|_| "unknown".into())
                )
            }
            Err(error) => format!("{name}: unreadable: {error}"),
        })
        .collect();
    section(&mut out, "flight", Some(&segments.join("\n")));
    section(
        &mut out,
        "data",
        Some(&data_lines(report, discovery).join("\n")),
    );
    Ok(out)
}

/// The crash's message (what comes before its first blank line) and the
/// first `BACKTRACE_LINES` lines of its backtrace.
fn crash_top(crash: &str) -> String {
    let message = crash.split("\n\n").next().unwrap_or("");
    let backtrace: Vec<&str> = crash
        .split_once("backtrace:\n")
        .map(|(_, rest)| rest.lines().take(BACKTRACE_LINES).collect())
        .unwrap_or_else(Vec::new);
    format!("{message}\nbacktrace:\n{}", backtrace.join("\n"))
}

fn last_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

/// The first and last tick a recording spans.
fn span(recording: &Recording) -> (u64, u64) {
    let first = recording.start.tick;
    (first, first + recording.ticks.len() as u64)
}

fn seconds(recording: &Recording) -> f64 {
    recording
        .ticks
        .iter()
        .map(|tick| f64::from(tick.msec))
        .sum::<f64>()
        / 1000.0
}

/// Each `data.txt` line's level digest against this install's: the same
/// data, or another edition or a mod.
fn data_lines(report: &Report, discovery: &vfs::Discovery) -> Vec<String> {
    let segments = report.segments();
    let mut installers = Installers::default();
    report
        .text("data.txt")
        .unwrap_or_else(String::new)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (name, level, digest) = (words.next()?, words.next()?, words.next()?);
            let verdict = match segments.iter().find(|(segment, _)| *segment == name) {
                None => "not checked: the segment is not in the report".to_string(),
                Some((_, bytes)) => match sim::flight::read(bytes) {
                    Err(error) => format!("not checked: {error}"),
                    Ok(recording) => match installers.level(
                        discovery,
                        &recording.level,
                        recording.start.place.as_ref(),
                        recording.start.seats(),
                    ) {
                        Err(error) => format!("not checked: {error}"),
                        Ok(here) if hex(&here.digest()) == digest => {
                            "same data as this install".to_string()
                        }
                        Ok(_) => "other data (edition or mod)".to_string(),
                    },
                },
            };
            Some(format!("{name} {level}: {verdict}"))
        })
        .collect()
}

/// The replay's account, whether it matched, and the segment replayed
/// (unpacked).
fn replay_text(
    discovery: &vfs::Discovery,
    zip: &Path,
    to_tick: Option<u64>,
    ignore_data: bool,
) -> Result<(String, bool, PathBuf), String> {
    let report = Report::open(zip)?;
    let folder = unpacked(zip);
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    for (name, bytes) in &report.0 {
        let path = folder.join(name);
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let (name, bytes) = *report
        .segments()
        .last()
        .ok_or("the report holds no flight recorder segment")?;
    let mut out = String::new();
    let _ = writeln!(out, "unpacked to {}", folder.display());
    let made = sim::flight::build(bytes).map_err(|error| format!("{name}: {error}"))?;
    let here = env!("KJK_GIT_SHA");
    if made != here {
        let _ = writeln!(
            out,
            "this report was made with {made}; check out that commit and build --release for an exact replay (this build is {here})"
        );
    }
    if cfg!(debug_assertions) {
        out.push_str("this is a debug build: a player's release recording may diverge in it\n");
    }
    let mut recording = sim::flight::read(bytes).map_err(|error| format!("{name}: {error}"))?;
    if let Some(to) = to_tick {
        cut(&mut recording, to);
    }
    let mut installers = Installers::default();
    let (id, place, seats) = (
        &recording.level,
        recording.start.place.as_ref(),
        recording.start.seats(),
    );
    let level = if ignore_data {
        installers.level(discovery, id, place, seats)
    } else {
        installers.level_named(discovery, id, place, seats)
    }
    .map_err(|error| format!("{name}: refused: {error} (--ignore-data replays it anyway)"))?;
    let result = recording.play_checked(
        &level,
        &mut |announcement| {
            installers
                .get(discovery, &announcement.game)?
                .level_for(announcement)
        },
        &mut |_| {},
        !ignore_data,
    );
    let (first, last) = span(&recording);
    let matched = match &result {
        Ok(end) => {
            let _ = writeln!(
                out,
                "{name}: matched, {} from tick {first} to tick {last}, ends at {}",
                recording.level.name,
                end.hash()
            );
            true
        }
        Err(divergence) => {
            let _ = writeln!(
                out,
                "{name}: DIVERGED by tick {}: recorded {}, replayed {}{}",
                divergence.tick,
                divergence.recorded,
                divergence.replayed,
                divergence
                    .reason
                    .as_ref()
                    .map_or_else(String::new, |reason| format!(" ({reason})"))
            );
            false
        }
    };
    Ok((out, matched, folder.join(name)))
}

/// `recording` stopped at its last checkpoint at or before tick `to`, the
/// last place whose hash it knows.
fn cut(recording: &mut Recording, to: u64) {
    recording.checkpoints.retain(|(at, _)| *at <= to);
    let (end_tick, end) = recording
        .checkpoints
        .last()
        .copied()
        .unwrap_or((recording.start.tick, recording.start.hash()));
    recording
        .ticks
        .truncate((end_tick - recording.start.tick) as usize);
    recording
        .installs
        .retain(|install| install.tick <= end_tick);
    recording.end = end;
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Where `replay` unpacks the report at `zip`.
fn unpacked(zip: &Path) -> PathBuf {
    let stem = zip
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    std::env::temp_dir().join(stem)
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
