//! The probes of `crash` (`kjk sim probes`, group "crash").

use super::*;
use sim::probe::{Checks, check};

/// In a folder of its own under the system's temporary one:
/// - `crash.txt` holds the message, place, thread, backtrace, facts and
///   breadcrumbs, and says when the breadcrumbs could not be read;
/// - the breadcrumbs read as none while another thread holds them;
/// - which earlier run ended unexpectedly;
/// - the pruning keeps the newest runs and recent crashes.
pub fn probes() -> Checks {
    let root = std::env::temp_dir().join(format!("kjk-crash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut checks = vec![
        ("crash_holds_everything", holds_everything(&root.join("everything"))),
        ("crash_without_crumbs", without_crumbs(&root.join("without"))),
        ("crumbs_while_held", while_held()),
    ];
    checks.extend(unexpected(&root.join("runs")));
    checks.push(("prune_keeps_crashes", prune(&root.join("prune"))));
    let _ = std::fs::remove_dir_all(&root);
    checks
}

fn fresh(folder: &Path) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())
}

fn crash_text(folder: &Path, crumbs: Option<&Breadcrumbs>) -> Result<String, String> {
    fresh(folder)?;
    write_crash(folder, "it broke", "src/x.rs:1:2", "main", "0: frame", "kjk 9.9\n", crumbs)
        .map_err(|error| error.to_string())?;
    std::fs::read_to_string(folder.join(CRASH)).map_err(|error| error.to_string())
}

fn holds_everything(folder: &Path) -> Result<String, String> {
    let crumbs = Breadcrumbs {
        tick: 42,
        module: "end_m01aa".into(),
        area: "Command Module".into(),
        conversation: Some(("end_trask".into(), 7)),
        party: vec!["trask".into()],
        ..Breadcrumbs::default()
    };
    let text = crash_text(folder, Some(&crumbs))?;
    let wanted = [
        "panic: it broke",
        "at src/x.rs:1:2",
        "thread main",
        "kjk 9.9",
        "tick 42",
        "module end_m01aa (Command Module)",
        "conversation end_trask node 7",
        "party trask",
        "0: frame",
    ];
    let missing: Vec<&str> = wanted.into_iter().filter(|line| !text.contains(line)).collect();
    check(missing.is_empty(), format!("missing {missing:?}"))
}

fn without_crumbs(folder: &Path) -> Result<String, String> {
    let text = crash_text(folder, None)?;
    check(text.contains("breadcrumbs: unavailable"), "the breadcrumbs' absence said".into())
}

fn while_held() -> Result<String, String> {
    let (held, release) = (std::sync::Barrier::new(2), std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        scope.spawn(|| {
            note(|_| {
                held.wait();
                release.wait();
            });
        });
        held.wait();
        let read = crumbs();
        release.wait();
        check(read.is_none(), format!("read while held: {read:?}"))
    })
}

/// The cases of `unexpected_end`: a folder's markers and whether it is the
/// one found.
fn unexpected(runs: &Path) -> Checks {
    let cases: [(&'static str, &[&str], bool); 6] = [
        ("unexpected_crashed", &[PLAYER, CRASH], true),
        ("unexpected_killed", &[PLAYER], true),
        ("unexpected_ended", &[PLAYER, ENDED], false),
        ("unexpected_seen", &[PLAYER, CRASH, SEEN], false),
        ("unexpected_not_a_player", &[CRASH], false),
        ("unexpected_current", &[PLAYER, CRASH], false),
    ];
    cases
        .iter()
        .enumerate()
        .map(|(n, (name, markers, found))| {
            let runs = runs.join(n.to_string());
            let folder = runs.join("20261028-120000-1");
            let current = if *name == "unexpected_current" { folder.clone() } else { runs.join("20261028-130000-2") };
            let result = fresh(&folder).and_then(|()| fresh(&current)).and_then(|()| {
                markers.iter().for_each(|marker| mark(&folder, marker));
                let got = unexpected_end(&runs, &current);
                check(got.as_deref() == found.then_some(folder.as_path()), format!("{got:?}"))
            });
            (*name, result)
        })
        .collect()
}

/// 12 plain runs and 3 crashed older ones, one of them crashed 31 days
/// ago: 10 plain and the 2 recent crashes stay.
fn prune(runs: &Path) -> Result<String, String> {
    let now = SystemTime::now();
    let mut folders = Vec::new();
    for n in 0..15 {
        let folder = runs.join(format!("20261028-1200{n:02}-1"));
        fresh(&folder)?;
        if n < 3 {
            crash_text(&folder, None)?;
            if n == 0 {
                let month = now - Duration::from_secs(31 * 86_400);
                std::fs::File::options()
                    .write(true)
                    .open(folder.join(CRASH))
                    .and_then(|file| file.set_modified(month))
                    .map_err(|error| error.to_string())?;
            }
        }
        folders.push(folder);
    }
    let kept = kept(folders.clone(), 10, now);
    let wanted: Vec<PathBuf> = folders[1..3].iter().chain(&folders[5..]).cloned().collect();
    check(kept == wanted, format!("kept {} of 15", kept.len()))
}
