//! The probes of `kjk report` (`kjk sim probes`, group "report verbs"), on
//! reports made here from the pinned run: a flight segment of the arena,
//! zipped as F12 zips one.

use super::*;
use kjk_game::report::{Identity, Part};
use sim::StepReason;
use sim::determinism::{Run, Setup, pilot};
use sim::flight::FlightWriter;
use sim::probe::{Checks, check};
use std::io::Write as _;

/// Ticks the fixture's segment holds: two whole chunks and a part.
const TICKS: u64 = 130;
const NOTE: &str = "probe note: the door would not open";

/// A report of the pinned run, its segment's level digest bent when
/// `bent`, written to a file of its own; the file.
fn fixture(name: &str, bent: bool) -> PathBuf {
    let setup = Setup::pinned();
    let mut run = Run::new(&setup);
    let mut id = run.world.level().id();
    if bent {
        id.digest[0] ^= 0xff;
    }
    let (mut flight, mut segment) = FlightWriter::new(
        run.rate.hz(),
        &run.last,
        &id,
        run.world.script_state(),
        env!("KJK_GIT_SHA"),
    );
    for tick in 1..=TICKS {
        let input = pilot(setup.pilot, tick, setup.world.seats);
        let msec = run.rate.msec(tick);
        let after = run.step(&input, StepReason::Authority).clone();
        segment.extend(flight.tick(msec, &input, &after).into_iter().flatten());
    }
    segment.extend(flight.flush(&run.last));
    let text = |name: &str, text: String| Part {
        name: name.into(),
        bytes: text.into_bytes(),
        text: true,
    };
    let backtrace: String = (1..=20).map(|n| format!("  {n}: frame {n}\n")).collect();
    let log: String = (1..=50).map(|n| format!("log line {n}\n")).collect();
    let parts = vec![
        text("note.txt", NOTE.into()),
        text("system.txt", kjk_game::crash::facts()),
        text(
            "crash.txt",
            format!(
                "panic: probe panic\nat here.rs:1:1\nthread main\n\nfacts\nbacktrace:\n{backtrace}"
            ),
        ),
        text("kjk.log", log),
        text("breadcrumbs.txt", "tick 130\nmodule arena ()\n".into()),
        text(
            "data.txt",
            format!("flight-1.kjkf {} {}\n", id.name, hex(&id.digest)),
        ),
        Part {
            name: "flight-1.kjkf".into(),
            bytes: segment,
            text: false,
        },
    ];
    let (zipped, _) = kjk_game::report::zip(parts, &Identity::default(), usize::MAX);
    let path = std::env::temp_dir().join(format!(
        "kjk-report-probe-{name}-{}.zip",
        std::process::id()
    ));
    let written = std::fs::File::create(&path).and_then(|mut file| file.write_all(&zipped));
    if let Err(error) = written {
        eprintln!("{}: {error}", path.display());
    }
    path
}

/// `what` when `ok`, else `what` and the `text` that missed it.
fn said(ok: bool, what: &str, text: &str) -> Result<String, String> {
    check(
        ok,
        if ok {
            what.to_string()
        } else {
            format!("{what}:\n{text}")
        },
    )
}

pub fn probes() -> Checks {
    let discovery = crate::discover();
    let (good, bent) = (fixture("good", false), fixture("bent", true));
    let shown = |zip: &Path| Report::open(zip).and_then(|report| show_text(&report, &discovery));
    let replayed =
        |zip: &Path, to: Option<u64>, ignore: bool| replay_text(&discovery, zip, to, ignore);
    let checks = vec![
        (
            "report_show",
            shown(&good).and_then(|text| {
                let has = |part: &str| text.contains(part);
                said(
                    has(NOTE)
                        && has("same build")
                        && has("same data")
                        && has("frame 15")
                        && !has("frame 16")
                        && has("log line 50")
                        && !has("log line 10\n")
                        && has(&format!("ticks 0..{TICKS}")),
                    "the note, build, data, 15 backtrace lines, 40 log lines and the span",
                    &text,
                )
            }),
        ),
        (
            "report_show_other_data",
            shown(&bent).and_then(|text| {
                said(
                    text.contains("other data (edition or mod)"),
                    "other data",
                    &text,
                )
            }),
        ),
        (
            "report_replay_matches",
            replayed(&good, None, false)
                .and_then(|(text, matched, _)| said(matched, "matched to its end", &text)),
        ),
        (
            "report_replay_to_tick",
            replayed(&good, Some(100), false).and_then(|(text, matched, _)| {
                said(
                    matched && text.contains("to tick 60"),
                    "--to-tick 100 stops at tick 60",
                    &text,
                )
            }),
        ),
        (
            "report_replay_other_data_refused",
            match replayed(&bent, None, false) {
                Ok((text, _, _)) => said(false, "refused", &text),
                Err(error) => said(error.contains("--ignore-data"), "refused", &error),
            },
        ),
        (
            "report_replay_ignore_data",
            replayed(&bent, None, true)
                .and_then(|(text, matched, _)| said(matched, "replayed with --ignore-data", &text)),
        ),
    ];
    for zip in [good, bent] {
        let _ = std::fs::remove_file(&zip);
        let _ = std::fs::remove_dir_all(unpacked(&zip));
    }
    checks
}
