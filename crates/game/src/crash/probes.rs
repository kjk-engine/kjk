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
        (
            "crash_holds_everything",
            holds_everything(&root.join("everything")),
        ),
        (
            "crash_without_crumbs",
            without_crumbs(&root.join("without")),
        ),
        ("crumbs_while_held", while_held()),
    ];
    checks.push(("lost_device_told", lost_device_told(&root.join("lost"))));
    checks.push((
        "lost_device_render_facts",
        lost_device_render_facts(&root.join("facts")),
    ));
    checks.extend(unexpected(&root.join("runs")));
    checks.push(("prune_keeps_crashes", prune(&root.join("prune"))));
    checks.push(("software_adapter_refused", software_adapter()));
    checks.push(("long_pause_clamped", long_pause()));
    checks.push(("native_crash_told", native_crash(&root.join("native"))));
    let _ = std::fs::remove_dir_all(&root);
    checks
}

fn fresh(folder: &Path) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())
}

fn crash_text(folder: &Path, crumbs: Option<&Breadcrumbs>) -> Result<String, String> {
    fresh(folder)?;
    write_crash(
        folder,
        "it broke",
        "src/x.rs:1:2",
        "main",
        "0: frame",
        "kjk 9.9\n",
        crumbs,
    )
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
        target: Some(aimed("Outcast Corpse (Empty)", "tar05_promcorpse", 1.48)),
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
        "target Outcast Corpse (Empty) (tag tar05_promcorpse, 1.5 m)",
        "0: frame",
    ];
    let missing: Vec<&str> = wanted
        .into_iter()
        .filter(|line| !text.contains(line))
        .collect();
    check(missing.is_empty(), format!("missing {missing:?}"))
}

fn without_crumbs(folder: &Path) -> Result<String, String> {
    let text = crash_text(folder, None)?;
    check(
        text.contains("breadcrumbs: unavailable"),
        "the breadcrumbs' absence said".into(),
    )
}

/// A crash after the render device was lost is told as the driver's, and
/// any other crash is not.
fn lost_device_told(folder: &Path) -> Result<String, String> {
    let plain = folder.join("plain");
    crash_text(&plain, None)?;
    let lost = folder.join("lost");
    fresh(&lost)?;
    write_crash(
        &lost,
        &lost_line("Unknown driver at fault"),
        "the render device",
        "main",
        "",
        "kjk 9.9\n",
        None,
    )
    .map_err(|error| error.to_string())?;
    let told = (lost_device_in(&plain), lost_device_in(&lost));
    check(told == (false, true), format!("plain, lost: {told:?}"))
}

/// The crash of a lost render device lists the render facts of the last
/// frames below the build's facts.
fn lost_device_render_facts(folder: &Path) -> Result<String, String> {
    use crate::render_facts::{Fact, note, push};
    fresh(folder)?;
    note(Fact::Capsules, 77);
    push(1);
    write_crash(
        folder,
        &lost_line("Unknown driver at fault"),
        "the render device",
        "main",
        "",
        &with_render_facts("kjk 9.9\n"),
        None,
    )
    .map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(folder.join(CRASH)).map_err(|error| error.to_string())?;
    check(
        text.contains("kjk 9.9\nrender facts, a line for each") && text.contains("capsules 77"),
        text,
    )
}

/// A crash that is no panic (issue 747) is told by its exception's code,
/// name, address and module, and an unknown code or module still reads.
fn native_crash(folder: &Path) -> Result<String, String> {
    fresh(folder)?;
    let line = native::native_line(0xC000_0005, 0x7ffa_1234, Some("nvoglv64.dll"));
    write_crash(folder, &line, "native code", "main", "", "kjk 9.9\n", None)
        .map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(folder.join(CRASH)).map_err(|error| error.to_string())?;
    let unknown = native::native_line(0x1234_5678, 0, None);
    let overflow = native::exception_name(0xC000_00FD);
    check(
        text.contains(
            "native crash: exception 0xC0000005 (access violation) at 0x7ffa1234 in nvoglv64.dll",
        ) && unknown.contains("(unknown exception)")
            && unknown.contains("in an unknown module")
            && overflow == "stack overflow"
            && !lost_device_in(folder),
        format!("{text}\n{unknown}\n{overflow}"),
    )
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
            let current = if *name == "unexpected_current" {
                folder.clone()
            } else {
                runs.join("20261028-130000-2")
            };
            let result = fresh(&folder)
                .and_then(|()| fresh(&current))
                .and_then(|()| {
                    markers.iter().for_each(|marker| mark(&folder, marker));
                    let got = unexpected_end(&runs, &current);
                    check(
                        got.as_deref() == found.then_some(folder.as_path()),
                        format!("{got:?}"),
                    )
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

/// A run with a window refuses a software renderer and nothing else. A
/// headless run and a run with `KJK_SOFTWARE_RENDERER=1` take it.
fn software_adapter() -> Result<String, String> {
    use wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu, Other, VirtualGpu};
    // Each case: the adapter, headless, allowed, and whether it is refused.
    let cases = [
        (Cpu, false, false, true),
        (Cpu, true, false, false),
        (Cpu, false, true, false),
        (IntegratedGpu, false, false, false),
        (DiscreteGpu, false, false, false),
        (VirtualGpu, false, false, false),
        (Other, false, false, false),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(device, headless, allowed, refused)| {
            refuses_adapter(*device, *headless, *allowed) != *refused
        })
        .map(|case| format!("{case:?}"))
        .collect();
    check(wrong.is_empty(), format!("wrong for {wrong:?}"))
}

/// A pause of eight hours, as when a computer sleeps with the game running,
/// moves the game by one frame at most. Bevy's game clock counts at most
/// its `max_delta` of a frame's wall time, and the sim's clock makes up at
/// most `MAX_CATCH_UP` ticks of it. The next ordinary frame then steps as
/// many ticks as it would on a fresh clock, so no backlog of steps is left.
fn long_pause() -> Result<String, String> {
    let max_delta = bevy::time::Time::<bevy::time::Virtual>::default().max_delta();
    let frame = std::time::Duration::from_millis(16);
    let fresh = sim::Accumulator::new(sim::TickRate::default(), 0)
        .advance(frame)
        .len();
    let mut clock = sim::Accumulator::new(sim::TickRate::default(), 0);
    let slept = clock
        .advance(std::time::Duration::from_secs(8 * 60 * 60))
        .len();
    let next = clock.advance(frame).len();
    check(
        max_delta <= std::time::Duration::from_secs(1)
            && slept as u32 <= sim::MAX_CATCH_UP
            && next == fresh,
        format!(
            "max_delta {max_delta:?}, {slept} ticks after the pause, {next} the frame after \
             ({fresh} on a fresh clock)"
        ),
    )
}
