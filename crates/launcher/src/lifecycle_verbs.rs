//! `kjk lifecycle` (I14): a module planned, committed and installed again
//! from its announcement (`plan`), and runs that go between two modules and
//! count what is left behind (`cycle`).

use session::Request;
use sim::{Announcement, Level, SimWorld};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;
use vfs::{Discovery, Game};

use crate::lifecycle::*;

pub use crate::levels::level_of_place;

fn census_json(census: &sim::Census) -> serde_json::Value {
    serde_json::json!({ "live": census.live, "left_behind": census.stashed })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `kjk lifecycle plan`: plans a module (what it resolved, how long it
/// took), commits it into a new world as a host would, and installs it
/// again as a client from the host's announcement alone (through bytes):
/// the client's plan must match and reach the host's state. Then shows the
/// typed refusal a client with other data gets.
pub fn plan_verb(
    discovery: &Discovery,
    game: &str,
    module: &str,
    arrival: Option<&str>,
) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("lifecycle plan: use k1 or tsl");
        return ExitCode::from(2);
    };
    let (anims, anims_note) = crate::stage::anims(discovery);
    let installer = match Installer::new(discovery, game, Arc::new(anims)) {
        Ok(installer) => installer,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let request = installer.request(module, arrival, sim::WorldConfig::one_seat().seats);
    let host = match installer.prepare(&request, false) {
        Ok(prepared) => prepared,
        Err(error) => {
            outln!("lifecycle plan {}: refused: {error}", request.module);
            return ExitCode::FAILURE;
        }
    };
    let facts = &host.plan.facts;
    outln!(
        "lifecycle plan {} (arriving at {}): {anims_note}",
        request.module,
        request.arrival.as_deref().unwrap_or("the entry")
    );
    outln!(
        "  planned in {:.0} ms: archives {:.0}, module and objects {:.0}, collision {:.0} ({} brushes)",
        host.open_ms + host.plan_ms,
        host.open_ms,
        facts.read_ms,
        facts.collision_ms,
        facts.brushes
    );
    outln!(
        "  objects: {}",
        facts
            .counts
            .iter()
            .map(|(kind, count)| format!("{count} {kind}s"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    outln!(
        "  {} item and {} creature templates read in {:.0} ms, {} scripts in {:.0} ms",
        facts.items,
        facts.creatures,
        facts.items_ms,
        facts.library.unwrap_or(0),
        facts.script_ms
    );
    outln!(
        "  {} event scripts recorded (not run until I16), {} link overrides applied, loading screen {:?}",
        facts.scripts,
        facts.overridden,
        facts.load_screen
    );
    for (kind, tag, to, at) in &facts.exits {
        outln!("  exit: {kind} {tag} -> {to} at {:?}", at);
    }
    let module_data = host
        .plan
        .level
        .module()
        .cloned()
        .expect("a plan has its module");
    let doors: Vec<String> = module_data
        .objects()
        .filter(|object| object.kind == sim::module::ObjectKind::Door)
        .map(|door| {
            let opens = match door.opens {
                sim::module::Opens::Yes => "opens",
                sim::module::Opens::Locked => "locked",
                sim::module::Opens::Never => "static",
            };
            format!(
                "{} ({:.1}, {:.1}) {opens}{}",
                door.tag,
                door.position[0],
                door.position[1],
                if door.state > 0 { ", open" } else { "" }
            )
        })
        .collect();
    outln!("  doors: {}", doors.join("; "));

    // The host installs it.
    let mut world =
        SimWorld::new(sim::WorldConfig::one_seat(), Level::arena()).expect("one seat in the arena");
    let announcement = match host.plan.commit(&mut world) {
        Ok(announcement) => announcement,
        Err(error) => {
            outln!("  commit refused: {error}");
            return ExitCode::FAILURE;
        }
    };
    let after = world.snapshot();
    let bytes = announcement.encode();
    let census = after.census();
    let player = after.entities.iter().find_map(|entity| entity.player);
    outln!(
        "  host: committed; {} entities ({}), the player at {:?} units, looking {:.1}°",
        census.live_total(),
        census
            .live
            .iter()
            .map(|(kind, count)| format!("{count} {kind}"))
            .collect::<Vec<_>>()
            .join(", "),
        player.map(|p| p.origin.map(|v| (v * 10.0).round() / 10.0)),
        player.map_or(0.0, |p| p.view[1])
    );
    outln!(
        "  announcement ({} bytes, KJKA v2): {} {} at {:?}, tick {}, {} seats; module {}, collision {}, animations {}; after {}",
        bytes.len(),
        announcement.game,
        announcement.module,
        announcement.arrival,
        announcement.tick,
        announcement.seats,
        &hex(&announcement.digests.module)[..16],
        &announcement.digests.collision[..16.min(announcement.digests.collision.len())],
        &hex(&announcement.digests.anims)[..16],
        announcement.after.short()
    );

    // A client installs it from the announcement alone.
    let decoded = match Announcement::decode(&bytes) {
        Ok(decoded) => decoded,
        Err(error) => {
            outln!("  the announcement does not decode: {error}");
            return ExitCode::FAILURE;
        }
    };
    let client_request = Request::of(&decoded);
    let client = installer.prepare(&client_request, false);
    let (client_check, client_after) = match &client {
        Ok(client) => {
            let check = client.plan.check(&decoded);
            let mut world = SimWorld::new(sim::WorldConfig::one_seat(), Level::arena())
                .expect("one seat in the arena");
            let after = client
                .plan
                .commit(&mut world)
                .map_err(|error| error.to_string())
                .and_then(|_| {
                    let after = world.snapshot();
                    decoded
                        .check_after(&after)
                        .map(|()| after.hash())
                        .map_err(|mismatch| mismatch.to_string())
                });
            (check.map_err(|mismatch| mismatch.to_string()), after)
        }
        Err(error) => (Err(error.clone()), Err(error.clone())),
    };
    let reproduced = client_check.is_ok() && client_after.is_ok();
    outln!(
        "  client, from the announcement's bytes: plan {}; state after the install {}",
        match &client_check {
            Ok(()) => "matches".to_string(),
            Err(error) => format!("REFUSED ({error})"),
        },
        match &client_after {
            Ok(hash) => format!("the host's ({})", hash.short()),
            Err(error) => format!("DIFFERENT ({error})"),
        }
    );
    if let Ok(client) = &client {
        outln!(
            "  the client planned in {:.0} ms, its area {}",
            client.open_ms + client.plan_ms,
            if client.plan.facts.cached {
                "from the level cache (the host's build)"
            } else {
                "built again"
            }
        );
    }
    // A client whose collision differs is refused, and told what differs.
    let mut other = decoded.clone();
    other.digests.collision = "0".repeat(64);
    let refused = client.as_ref().ok().map(|client| client.plan.check(&other));
    outln!(
        "  a client with other collision data: {}",
        match &refused {
            Some(Err(mismatch)) => format!("refused: {mismatch}"),
            Some(Ok(())) => "NOT refused".to_string(),
            None => "no plan".to_string(),
        }
    );
    let refused_ok = matches!(refused, Some(Err(sim::Mismatch::Collision { .. })));
    crate::runtime::write_facts(serde_json::json!({
        "verb": "lifecycle plan",
        "game": game.key(),
        "module": request.module,
        "arrival": request.arrival,
        "anims": anims_note,
        "open_ms": host.open_ms,
        "plan_ms": host.plan_ms,
        "read_ms": facts.read_ms,
        "collision_ms": facts.collision_ms,
        "brushes": facts.brushes,
        "objects": facts.counts.iter().map(|(k, n)| (k.to_string(), n)).collect::<std::collections::BTreeMap<_, _>>(),
        "scripts": facts.scripts,
        "link_overrides": facts.overridden,
        "load_screen": facts.load_screen,
        "exits": facts.exits.iter().map(|(kind, tag, to, at)| serde_json::json!({
            "kind": kind, "tag": tag, "module": to, "arrival": at,
        })).collect::<Vec<_>>(),
        "census": census_json(&census),
        "announcement": {
            "bytes": bytes.len(),
            "game": announcement.game,
            "module": announcement.module,
            "arrival": announcement.arrival,
            "tick": announcement.tick,
            "seats": announcement.seats,
            "module_digest": hex(&announcement.digests.module),
            "collision_digest": announcement.digests.collision,
            "anims_digest": hex(&announcement.digests.anims),
            "after": announcement.after.to_string(),
        },
        "client_reproduced": reproduced,
        "client_with_other_collision_refused": refused_ok,
    }));
    if reproduced && refused_ok {
        outln!("lifecycle plan: passed");
        ExitCode::SUCCESS
    } else {
        outln!("lifecycle plan: FAILED");
        ExitCode::FAILURE
    }
}

/// What `cycle` saw after one install.
struct Visit {
    module: String,
    tick: u64,
    census: sim::Census,
    next_id: u32,
    prepare_ms: f64,
    commit_ms: f64,
    hash: String,
}

/// One seat walking forward and turning, using what it looks at now and
/// then: a pure function of the tick.
fn wander_command(tick: u64) -> sim::Command {
    let yaw = (tick % 2000) as f32 * 0.18;
    sim::Command {
        angles: sim::moves::view_angles(0.0, yaw),
        forward: if tick % 400 < 300 { 127 } else { 0 },
        right: 0,
        up: 0,
        buttons: if tick % 97 < 5 { sim::buttons::USE } else { 0 },
        ..sim::Command::default()
    }
}

/// Installs `a` and `b` in turn `times` times each, `ticks` ticks apart,
/// recording; the visits, the recording and the end.
fn run_cycle(
    installer: &Installer,
    a: &str,
    b: &str,
    times: u32,
    ticks: u64,
) -> Result<(Vec<Visit>, sim::Recording, SimWorld), String> {
    let rate = sim::TickRate::default();
    let mut world =
        SimWorld::new(sim::WorldConfig::one_seat(), Level::arena()).expect("one seat in the arena");
    let mut recorder: Option<sim::Recorder> = None;
    let mut visits = Vec::new();
    let mut last = world.snapshot();
    let order = std::iter::once(a).chain((0..times).flat_map(|_| [b, a]));
    for module in order {
        let started = Instant::now();
        let prepared = installer.prepare(&installer.request_from(&last, module, None), false)?;
        let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let announcement = prepared
            .plan
            .commit(&mut world)
            .map_err(|error| error.to_string())?;
        let commit_ms = started.elapsed().as_secs_f64() * 1000.0;
        last = world.snapshot();
        match &mut recorder {
            // The recording starts in the first module.
            None => {
                recorder = Some(
                    sim::Recorder::new(rate.hz(), last.clone(), world.level().id(), 100)
                        .with_script(world.script_state().clone()),
                )
            }
            Some(recorder) => recorder.install(announcement.clone()),
        }
        visits.push(Visit {
            module: module.to_string(),
            tick: last.tick,
            census: last.census(),
            next_id: next_id(&last),
            prepare_ms,
            commit_ms,
            hash: announcement.after.to_string(),
        });
        for _ in 0..ticks {
            let tick = last.tick + 1;
            let mut input = sim::TickInput::default();
            input.insert(sim::Seat(0), wander_command(tick));
            last = sim::step(
                &mut world,
                tick,
                &input,
                rate.msec(tick),
                sim::StepReason::Authority,
            );
            if let Some(recorder) = &mut recorder {
                recorder.record(rate.msec(tick), &input, &last);
            }
        }
    }
    let recording = recorder.ok_or("nothing installed")?.finish(&last);
    Ok((visits, recording, world))
}

/// The id the next new entity would get: it grows only when entities are
/// made, so a module revisited from where it was left keeps it.
fn next_id(snapshot: &sim::Snapshot) -> u32 {
    let highest = snapshot
        .entities
        .iter()
        .chain(snapshot.stash.iter().flat_map(|stashed| &stashed.entities))
        .map(|entity| entity.id.0 + 1)
        .max();
    highest.unwrap_or(0)
}

/// `kjk lifecycle cycle`: installs two modules in turn, stepping a player
/// about in each, and checks nothing leaks: every visit of a module has
/// the same entities, the modules left keep their objects (and no more),
/// no entity is made after the first visits; the whole run again gives
/// the same hashes, its recording replays (every install replanned from
/// its announcement), and a save taken at the end loads into a level
/// planned from the save alone and steps on the same.
pub fn cycle_verb(
    discovery: &Discovery,
    game: &str,
    a: &str,
    b: &str,
    times: u32,
    ticks: u64,
) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("lifecycle cycle: use k1 or tsl");
        return ExitCode::from(2);
    };
    let (anims, anims_note) = crate::stage::anims(discovery);
    let installer = match Installer::new(discovery, game, Arc::new(anims)) {
        Ok(installer) => installer,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    outln!("lifecycle cycle: {a} <-> {b}, {times} times, {ticks} ticks in each; {anims_note}");
    let started = Instant::now();
    let (visits, recording, mut world) = match run_cycle(&installer, a, b, times, ticks) {
        Ok(run) => run,
        Err(error) => {
            outln!("  refused: {error}");
            return ExitCode::FAILURE;
        }
    };
    outln!(
        "  {:>2}  {:<10} {:>6}  {:>5}  {:>7}  {:>8}  {:>7}  {}",
        "#",
        "module",
        "tick",
        "live",
        "left",
        "next id",
        "plan ms",
        "by kind (live)"
    );
    for (index, visit) in visits.iter().enumerate() {
        outln!(
            "  {index:>2}  {:<10} {:>6}  {:>5}  {:>7}  {:>8}  {:>7.0}  {}",
            visit.module,
            visit.tick,
            visit.census.live_total(),
            visit.census.stashed_total(),
            visit.next_id,
            visit.prepare_ms,
            visit
                .census
                .live
                .iter()
                .map(|(kind, count)| format!("{kind} {count}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    // Leaks: each module's visits alike, the left-behind bounded, no new
    // entities after both were visited once.
    let same_visits = |module: &str| {
        let counts: Vec<&sim::Census> = visits
            .iter()
            .filter(|visit| visit.module == module)
            .map(|visit| &visit.census)
            .collect();
        counts.windows(2).all(|pair| pair[0].live == pair[1].live)
    };
    let alike = same_visits(a) && same_visits(b);
    let settled_ids = visits
        .iter()
        .skip(2)
        .all(|visit| visit.next_id == visits[1].next_id);
    let left_alike = |module: &str| {
        let left: Vec<usize> = visits
            .iter()
            .filter(|visit| visit.module == module)
            .skip(usize::from(module == a))
            .map(|visit| visit.census.stashed_total())
            .collect();
        left.windows(2).all(|pair| pair[0] == pair[1])
    };
    let bounded = visits
        .iter()
        .skip(1)
        .all(|visit| visit.census.stashed.len() == 1)
        && left_alike(a)
        && left_alike(b);
    outln!(
        "  every visit of a module alike: {}; left-behind modules: {} (of {} and {} objects); new entities after the first two visits: {}",
        if alike { "yes" } else { "NO" },
        visits.last().map_or(0, |v| v.census.stashed.len()),
        visits.first().map_or(0, |v| v.census.live_total()),
        visits.get(1).map_or(0, |v| v.census.live_total()),
        if settled_ids { "none" } else { "SOME" }
    );
    // The same run again.
    let again = run_cycle(&installer, a, b, times, ticks);
    let repeated = again
        .as_ref()
        .is_ok_and(|(_, other, _)| other.end == recording.end);
    outln!(
        "  the whole run again: {}",
        if repeated { "same hashes" } else { "DIFFERENT" }
    );
    // The recording, every install replanned from its announcement.
    let start_level = recording
        .start
        .place
        .as_ref()
        .ok_or("no module".to_string())
        .and_then(|place| level_of_place(&installer, place, recording.start.seats()));
    let replayed = start_level.and_then(|level| {
        recording
            .play_with(&level, &mut |announcement| {
                installer.level_for(announcement)
            })
            .map_err(|divergence| {
                format!(
                    "diverged at tick {}: {:?}",
                    divergence.tick, divergence.reason
                )
            })
    });
    let replay_same = replayed
        .as_ref()
        .is_ok_and(|end| end.hash() == recording.end);
    let recording_bytes = recording.encode().len();
    outln!(
        "  recording ({} ticks, {} installs, {recording_bytes} bytes) replayed: {}",
        recording.ticks.len(),
        recording.installs.len(),
        match &replayed {
            Ok(_) if replay_same => "same".to_string(),
            Ok(_) => "DIFFERENT".to_string(),
            Err(error) => error.clone(),
        }
    );
    // A save at the end, loaded into a level planned from the save.
    let end = world.snapshot();
    let save = sim::Save {
        hz: sim::TICK_HZ,
        level: world.level().id(),
        snapshot: end.clone(),
        script: world.script_state().clone(),
    }
    .encode();
    let loaded = sim::Save::decode(&save)
        .map_err(|error| error.to_string())
        .and_then(|save| {
            let place = save.snapshot.place.clone().ok_or("no module")?;
            let level = level_of_place(&installer, &place, save.snapshot.seats())?;
            save.level
                .check(&level)
                .map_err(|error| error.to_string())?;
            let mut loaded = SimWorld::from_snapshot(&save.snapshot, level)
                .map_err(|error| error.to_string())?;
            loaded.set_script_state(save.script.clone());
            Ok(loaded)
        });
    let rate = sim::TickRate::default();
    let continued = loaded.map(|mut other| {
        let (mut x, mut y) = (end.clone(), end.clone());
        for _ in 0..500 {
            let tick = x.tick + 1;
            let mut input = sim::TickInput::default();
            input.insert(sim::Seat(0), wander_command(tick));
            x = sim::step(
                &mut world,
                tick,
                &input,
                rate.msec(tick),
                sim::StepReason::Authority,
            );
            y = sim::step(
                &mut other,
                tick,
                &input,
                rate.msec(tick),
                sim::StepReason::Authority,
            );
        }
        x.hash() == y.hash()
    });
    outln!(
        "  saved at the end ({} bytes), loaded into a level planned from the save, 500 ticks on: {}",
        save.len(),
        match &continued {
            Ok(true) => "same".to_string(),
            Ok(false) => "DIFFERENT".to_string(),
            Err(error) => format!("refused: {error}"),
        }
    );
    let passed =
        alike && settled_ids && bounded && repeated && replay_same && continued == Ok(true);
    outln!(
        "  {:.1} s; lifecycle cycle: {}",
        started.elapsed().as_secs_f64(),
        if passed { "passed" } else { "FAILED" }
    );
    crate::runtime::write_facts(serde_json::json!({
        "verb": "lifecycle cycle",
        "game": game.key(),
        "modules": [a, b],
        "times": times,
        "ticks": ticks,
        "anims": anims_note,
        "visits": visits.iter().map(|visit| serde_json::json!({
            "module": visit.module,
            "tick": visit.tick,
            "census": census_json(&visit.census),
            "next_id": visit.next_id,
            "prepare_ms": visit.prepare_ms,
            "commit_ms": visit.commit_ms,
            "hash": visit.hash,
        })).collect::<Vec<_>>(),
        "visits_alike": alike,
        "no_new_entities_after_first_visits": settled_ids,
        "left_behind_bounded": bounded,
        "repeated_same": repeated,
        "replay_same": replay_same,
        "recording_bytes": recording_bytes,
        "save_bytes": save.len(),
        "save_continues_same": continued == Ok(true),
        "end": recording.end.to_string(),
        "passed": passed,
    }));
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
