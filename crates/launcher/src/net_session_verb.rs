//! `kjk net session`: whole sessions in virtual time (`net::session::probes`),
//! each a host and clients over a `SimNet` with netem, from a seed: the
//! determinism world's (`net::session::sessions`: lan, wan, soak, hostile,
//! reconnect, save_copy, hold, reload), judged by its checks
//! (`sessions::checks`); with `hostile`, a million corrupted session
//! encodings through their decoders (nothing panics); and `end_m01aa` when
//! KOTOR is installed: two clients running through the door to `end_m01ab`
//! and back, twice, while the module's creatures fight; `end_m01aa_late`,
//! a third client joining the module planned for three.
//!
//! Each prints its checks and measures and the restore and re-step cost of
//! prediction (release timings), and writes `facts.json`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use net::session::clock::Margins;
use net::session::probes::{self, ClientReport, Hitches, Joiner, Report, Scenario, Setup};
use net::session::sessions::{self, Expect, Session};
use net::session::{Cause, InstallRequest, LevelRequest};
use serde_json::json;
use sim::combat::mind;
use sim::script::dialogue::{self, Pick};
use sim::{Command, Level, Seat, SimWorld, Snapshot};
use vfs::Game;

/// Ticks re-stepped per snapshot at 100 ms RTT (the design's figure).
const RESTEP: usize = 13;
/// Corrupted encodings through the session's decoders.
const MUTATIONS: u64 = 1_000_000;

pub fn session(discovery: &vfs::Discovery, only: Option<&str>, soak_minutes: u64) -> ExitCode {
    let mut played = sessions::determinism(soak_minutes);
    match kotor_sessions(discovery) {
        Ok(kotor) if kotor.is_empty() => {
            outln!("KOTOR not found: the end_m01aa sessions are skipped")
        }
        Ok(kotor) => played.extend(kotor),
        Err(error) => {
            eprintln!("net session: end_m01aa: {error}");
            return ExitCode::FAILURE;
        }
    }
    played.retain(|played| only.is_none_or(|only| played.setup.name.contains(only)));
    let started = Instant::now();
    let runs: Vec<(String, Option<Report>, f64, Expect)> = std::thread::scope(|scope| {
        let handles: Vec<_> = played
            .into_iter()
            .map(|mut played| {
                scope.spawn(move || {
                    let started = Instant::now();
                    let report = catch_unwind(AssertUnwindSafe(|| played.run())).ok();
                    let seconds = started.elapsed().as_secs_f64();
                    (played.setup.name, report, seconds, played.expect)
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    });
    outln!(
        "{} sessions in {:.1} s ({} threads)",
        runs.len(),
        started.elapsed().as_secs_f64(),
        runs.len()
    );
    let mut failed = 0;
    let mut facts = Vec::new();
    for (name, report, seconds, expect) in &runs {
        match report {
            Some(report) => {
                let (fails, fact) = print(report, *seconds, *expect);
                failed += fails + usize::from(!crossed(report));
                facts.push(fact);
            }
            None => {
                outln!("{name}: FAIL: the session panicked");
                failed += 1;
                facts.push(json!({ "name": name, "panicked": true }));
            }
        }
    }
    if only.is_none_or(|only| "hostile".contains(only)) {
        let started = Instant::now();
        let mutations = probes::decoder_mutations(MUTATIONS, 5);
        let pass = mutations.panics == 0 && mutations.broken == 0 && mutations.tried == MUTATIONS;
        outln!(
            "decoders ({:.1} s):
  {} corrupted session encodings (commands frames to a host too): {} tried, {} panics, {} still decoded, {} commands frames with ticks past their newest",
            started.elapsed().as_secs_f64(),
            if pass { "ok  " } else { "FAIL" },
            mutations.tried,
            mutations.panics,
            mutations.accepted,
            mutations.broken
        );
        failed += usize::from(!pass);
        facts.push(json!({
            "name": "decoders", "tried": mutations.tried,
            "panics": mutations.panics, "accepted": mutations.accepted,
            "broken": mutations.broken,
        }));
    }
    crate::runtime::write_facts(json!({
        "verb": "net session",
        "sessions": facts,
        "failed": failed,
    }));
    if failed == 0 {
        outln!("net session: all passed");
        ExitCode::SUCCESS
    } else {
        outln!("net session: {failed} FAILED");
        ExitCode::FAILURE
    }
}

/// Whether an `end_m01aa` session crossed the door as often as its players
/// run through it (`CROSSINGS`), printed; any other session passes.
fn crossed(report: &Report) -> bool {
    if !report.name.starts_with(MODULES[0]) {
        return true;
    }
    let pass = report.installs >= u64::from(CROSSINGS);
    outln!(
        "  {} crossings: {} installs for the players' {CROSSINGS} runs through the door",
        if pass { "ok  " } else { "FAIL" },
        report.installs
    );
    pass
}

/// Checks and measures of one session; the failures and the facts.
fn print(report: &Report, seconds: f64, expect: Expect) -> (usize, serde_json::Value) {
    outln!(
        "{} ({} virtual s, {} ticks, {} installs) in {seconds:.1} s:",
        report.name,
        report.seconds,
        report.ticks,
        report.installs
    );
    for event in &report.events {
        outln!("    {event}");
    }
    outln!(
        "    the host ended in {} with its avatars at {:?} m; {} steps asked for a transition",
        if report.place.is_empty() {
            "a stage"
        } else {
            &report.place
        },
        report.avatars,
        report.asks
    );
    let checks = sessions::checks(report, expect);
    for check in &checks {
        let mark = match (check.pass, check.binding) {
            (true, _) => "ok  ",
            (false, true) => "FAIL",
            (false, false) => "note",
        };
        outln!("  {mark} {}: {}", check.name, check.detail);
    }
    let failed = checks.iter().filter(|check| check.failed()).count();
    for (seat, ticks, mean, max) in &report.lags {
        outln!(
            "       host: seat {seat}'s view lag (C18): mean {mean:.1} ms, max {max} ms over {ticks} ticks"
        );
    }
    for view in &report.server.connections {
        let s = &view.stats;
        outln!(
            "       host → seat {}: {} snapshot frames ({} extra fragments), {} keyframes, {} reliable deltas, every {} ticks ({} changes); commands {} (slack mean {:.1} ms), late {}, substituted {}, none {}, dropped {}; save copy parts {}",
            view.seat.0,
            s.snapshots,
            s.fragments,
            s.keyframes,
            s.reliable_deltas,
            view.every,
            s.interval_changes,
            s.commands,
            s.slack_sum_us as f64 / s.commands.max(1) as f64 / 1e3,
            s.late,
            s.substituted,
            s.none,
            s.dropped,
            s.copy_parts
        );
    }
    for (index, client) in report.clients.iter().enumerate() {
        print_client(index, client);
    }
    let clock = || Instant::now().duration_since(*EPOCH).as_secs_f64() * 1e6;
    let (restore, per_tick) = probes::bench(&report.samples, RESTEP, &clock);
    let per_snapshot = restore + per_tick * RESTEP as f64;
    outln!(
        "       prediction cost (release, {} samples): restore {:.1} µs, re-step {:.1} µs a tick, {:.1} µs per snapshot at {RESTEP} ticks (restore {:.0}%)",
        report.samples.len(),
        restore,
        per_tick,
        per_snapshot,
        100.0 * restore / per_snapshot.max(1e-9)
    );
    let fact = json!({
        "name": report.name,
        "virtual_seconds": report.seconds,
        "ticks": report.ticks,
        "installs": report.installs,
        "corrupted": report.corrupted,
        "end": report.end.to_string(),
        "replay": match &report.replay {
            Ok(hash) => hash.to_string(),
            Err(error) => error.clone(),
        },
        "checks": checks.iter().map(|check| json!({
            "name": check.name, "pass": check.pass, "binding": check.binding, "detail": check.detail,
        })).collect::<Vec<_>>(),
        "restore_us": restore,
        "restep_us_per_tick": per_tick,
        "clients": report.clients.iter().map(client_facts).collect::<Vec<_>>(),
        "failed": failed,
    });
    (failed, fact)
}

/// Client `index`'s measures (its checks are in `sessions::checks`).
fn print_client(index: usize, client: &ClientReport) {
    let stats = &client.stats;
    outln!(
        "       client {index} bolts: {} ticks where the host had creature bolts prediction did not ({} bolts), {} ticks where players' bolts differ",
        stats.creature_bolt_ticks,
        stats.creature_bolts_unpredicted,
        stats.player_bolt_ticks
    );
    outln!(
        "       client {index} seat {}: rebuilt {} (keyframes {}, no baseline {}, other epoch {}, stale {}), host late {} substituted {}, rewinds {} (mean {:.1}, max {} ticks), corrections {} (snapped {}), starvations {} ({} frames), skipped {}, held {}",
        client.seat.map_or("-".into(), |s| s.0.to_string()),
        stats.rebuilt,
        stats.keyframes,
        stats.no_baseline,
        stats.other_epoch,
        stats.stale,
        stats.host_late,
        stats.host_substituted,
        stats.rewinds,
        stats.rewind_ticks as f64 / stats.rewinds.max(1) as f64,
        stats.rewind_max,
        stats.corrections,
        stats.snapped,
        stats.starvations,
        stats.starved_frames,
        stats.skipped,
        stats.held,
    );
    outln!(
        "       client {index} link: rtt {:.1} ms (min {:.1}), loss {:.1}%, up {:.0} B/s, down {:.0} B/s; memory max: ring {}, seat commands {}, reassembly {}, own {}, history {}, held {} B, window {} B",
        client.link.rtt_us as f64 / 1e3,
        client.link.rtt_min_us as f64 / 1e3,
        client.link.loss_ppm as f64 / 1e4,
        client.up_bps,
        client.down_bps,
        client.memory_max.ring,
        client.memory_max.seat_commands,
        client.memory_max.reassembly,
        client.memory_max.own,
        client.memory_max.history,
        client.memory_max.held_bytes,
        client.memory_max.window_bytes,
    );
}

fn client_facts(c: &ClientReport) -> serde_json::Value {
    let stats = &c.stats;
    let by_cause: serde_json::Map<_, _> = Cause::ALL
        .iter()
        .map(|cause| {
            (
                cause.name().to_string(),
                json!(stats.by_cause[*cause as usize]),
            )
        })
        .collect();
    let prediction = json!({
        "compared": stats.compared,
        "mismatches": stats.mismatches,
        "by_cause": by_cause,
        "host_late": stats.host_late,
        "host_substituted": stats.host_substituted,
        "creature_bolt_ticks": stats.creature_bolt_ticks,
        "creature_bolts_unpredicted": stats.creature_bolts_unpredicted,
        "player_bolt_ticks": stats.player_bolt_ticks,
        "rewinds": stats.rewinds,
        "rewind_mean": stats.rewind_ticks as f64 / stats.rewinds.max(1) as f64,
        "rewind_max": stats.rewind_max,
    });
    let clock = json!({
        "starvations": stats.starvations,
        "starved_frames": stats.starved_frames,
        "starts": c.starts.iter().map(|start| json!({
            "lasted_ms": start.lasted_us as f64 / 1e3,
            "converged_ms": start.converged_us.map(|us| us as f64 / 1e3),
        })).collect::<Vec<_>>(),
        "lead_ticks": c.clock.map(|c| c.lead),
        "delay_ticks": c.clock.map(|c| c.delay),
    });
    let memory = &c.memory_max;
    json!({
        "seat": c.seat.map(|s| s.0),
        "joined_at_s": c.joined_at_us as f64 / 1e6,
        "rebuilt": stats.rebuilt,
        "keyframes": stats.keyframes,
        "checked": c.checked,
        "wrong": c.wrong,
        "check_failures": stats.check_failures,
        "prediction": prediction,
        "clock": clock,
        "up_bytes_per_s": c.up_bps,
        "down_bytes_per_s": c.down_bps,
        "rtt_ms": c.link.rtt_us as f64 / 1e3,
        "memory_max": {
            "ring": memory.ring, "own": memory.own, "history": memory.history,
            "held_bytes": memory.held_bytes, "window_bytes": memory.window_bytes,
        },
    })
}

static EPOCH: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

// ---------------------------------------------------------------------------
// KOTOR: end_m01aa.

/// The modules the players run between, and how many times each crosses.
const MODULES: [&str; 2] = ["end_m01aa", "end_m01ab"];
const CROSSINGS: u32 = 4;

/// Two players arriving at `wp_player01` in `end_m01aa`, answering
/// Trask's opening, then running through the door to `end_m01ab`, back,
/// and on for as long as the session lasts, every crossing an install
/// through the D13 flow (the ready check, P16: both walk in). The module's
/// creatures fight as their factions have them (bolts fly).
struct Kotor {
    installer: Arc<crate::lifecycle::Installer>,
    seats: u8,
    /// The players `end_m01aa` is first planned for: 1 for a new game, more
    /// as a save made with more has it (I30 P21).
    players: u8,
    /// Plans by module and the players it is planned for (collision is the
    /// costly part): what a level cache keyed by module gives every machine
    /// in-process.
    plans: Vec<Level>,
    /// Per seat, the module its client last showed and the crossings so far.
    seen: std::collections::BTreeMap<u8, (String, u32)>,
    /// Per seat, its answer byte and the line it last answered.
    answers: std::collections::BTreeMap<u8, (u8, Option<Line>)>,
}

/// A line shown: its conversation, entry and when it was shown (sim ms).
type Line = (u32, u32, u64);

impl Kotor {
    /// `request`'s module, its encounters for its players (I30 P21).
    fn level_for(&mut self, request: &session::Request) -> Result<Level, String> {
        if let Some(level) = self.planned(&request.module, request.players) {
            return Ok(level.clone());
        }
        let level = self.installer.prepare(request, false)?.plan.level;
        self.plans.push(level.clone());
        Ok(level)
    }

    fn planned(&self, module: &str, players: u8) -> Option<&Level> {
        self.plans.iter().find(|level| {
            level
                .module()
                .is_some_and(|data| data.name == module && data.players == players)
        })
    }

    /// `seat`'s answer byte (`dialogue::answer`): in a conversation, each
    /// line answered once, as a player clicks through (Trask's opening
    /// holds the party at `wp_player01` until it is): its first reply, or,
    /// with none offered, skipped once a skip may end it. Held until the
    /// next answer.
    fn answer(&mut self, seat: Seat, view: &Snapshot) -> u8 {
        let (byte, answered) = self.answers.entry(seat.0).or_insert((0, None));
        let pick = view.talk_for(seat).and_then(|talk| {
            let line = (talk.id, talk.entry, talk.shown_ms);
            let pick = if !talk.auto && !talk.replies.is_empty() {
                Pick::Reply(0)
            } else if talk.until_ms != u64::MAX
                && view.time_ms >= talk.shown_ms + dialogue::SKIP_AFTER_MS
            {
                Pick::Skip
            } else {
                return None;
            };
            (*answered != Some(line)).then(|| {
                *answered = Some(line);
                pick
            })
        });
        if let Some(pick) = pick {
            *byte = dialogue::answer(*byte, pick);
        }
        *byte
    }
}

impl Scenario for Kotor {
    fn start(&mut self) -> (SimWorld, Level, LevelRequest) {
        let config = sim::WorldConfig {
            seed: 1,
            seats: self.seats,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: sim::combat::Sheet::dev_soldier(),
            guests: sim::Guests::Avatars,
        };
        let mut world = SimWorld::new(config, Level::arena()).expect("seats in the arena");
        let request = self
            .installer
            .request(MODULES[0], Some("wp_player01"), self.seats)
            .with_players(self.players);
        let level = self.level_for(&request).expect("end_m01aa plans");
        let after = world
            .install(level.clone(), Some("wp_player01"))
            .expect("end_m01aa installs");
        let announcement =
            sim::Announcement::new(&level, Some("wp_player01"), &after).expect("a module");
        (world, level, LevelRequest::Module(announcement))
    }

    /// Runs through the door to the other module, as `lifecycle-walk`
    /// does: east in `end_m01aa`, west (back) in `end_m01ab`, holding use;
    /// after `CROSSINGS` module changes it stands (and watches the fight).
    fn command(&mut self, seat: Seat, _tick: u64, view: Option<&Snapshot>) -> Option<Command> {
        let view = view?;
        let here = &view.place.as_ref()?.module;
        let me = view.own(seat)?.player?;
        let reply = self.answer(seat, view);
        let seen = self.seen.entry(seat.0).or_insert((here.clone(), 0));
        if seen.0 != *here {
            *seen = (here.clone(), seen.1 + 1);
        }
        if seen.1 >= CROSSINGS {
            return Some(Command {
                reply,
                ..mind::looking(&me, 0.0, me.view[1])
            });
        }
        let yaw = if here == MODULES[0] { 0.0 } else { 180.0 };
        Some(Command {
            forward: 127,
            buttons: sim::buttons::USE,
            reply,
            ..mind::looking(&me, 0.0, yaw)
        })
    }

    fn level(&mut self, request: &LevelRequest) -> Result<Level, String> {
        match request {
            LevelRequest::Module(announcement) => self.plan(&InstallRequest::of(announcement)),
            LevelRequest::Stage(id) => Err(format!("{}: not a module", id.name)),
        }
    }

    /// As a joined player plans (`net_join::planned`).
    fn plan(&mut self, request: &InstallRequest) -> Result<Level, String> {
        self.level_for(&crate::net_join::planned(request))
    }

    fn install_after(&mut self, snapshot: &Snapshot) -> Option<InstallRequest> {
        let transition = snapshot.transition.as_ref()?;
        let game = snapshot.place.as_ref()?.game.clone();
        Some(InstallRequest {
            game,
            module: transition.module.clone(),
            arrival: transition.tag.clone(),
            seats: snapshot.seats(),
            players: snapshot.players_for(&transition.module),
        })
    }
}

/// `end_m01aa`, and `end_m01aa_late`: the module planned for three, as a
/// save made with three has it, and a third player joining at 6 s who
/// plans it as announced (I30 P21; planned for one, it is refused). None
/// without KOTOR.
fn kotor_sessions(discovery: &vfs::Discovery) -> Result<Vec<Session>, String> {
    if discovery.get(Game::Kotor1).is_none() {
        return Ok(Vec::new());
    }
    let (anims, _) = crate::stage::anims(discovery);
    let installer = Arc::new(crate::lifecycle::Installer::new(
        discovery,
        Game::Kotor1,
        Arc::new(anims),
    )?);
    let setup = Setup {
        name: "end_m01aa".into(),
        clients: vec![
            Joiner {
                at_us: 0,
                frame_us: 16_667,
                plan_us: 400_000,
            },
            Joiner {
                at_us: 3_000,
                frame_us: 6_944,
                plan_us: 600_000,
            },
        ],
        conditions: probes::conditions(50, 5, 2, 3),
        seconds: 20,
        every: 2,
        seed: 4,
        load_wait_us: 30_000_000,
        host_plan_us: 500_000,
        corrupt_ppm: 0,
        cut: None,
        hold: None,
        host_frame_us: 0,
        hitches: Hitches::default(),
        margins: Margins::default(),
    };
    let late = Setup {
        name: "end_m01aa_late".into(),
        clients: [
            setup.clients.as_slice(),
            &[Joiner {
                at_us: 6_000_000,
                frame_us: 8_333,
                plan_us: 500_000,
            }],
        ]
        .concat(),
        seed: 5,
        ..setup.clone()
    };
    let session = |setup: Setup, seats: u8, players: u8| Session {
        setup,
        scenario: Box::new(Kotor {
            installer: installer.clone(),
            seats,
            players,
            plans: Vec::new(),
            seen: std::collections::BTreeMap::new(),
            answers: std::collections::BTreeMap::new(),
        }),
        expect: Expect::Lossy,
    };
    Ok(vec![session(setup, 2, 1), session(late, 3, 3)])
}
