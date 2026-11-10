//! The sessions `kjk net session` plays on the determinism world and the
//! checks every session's report must pass, so the launcher and the
//! approved tests (`reload_in_place`, `hold_and_resume`) judge alike:
//! - `lan`: two clients on a clean 1 ms link, the proving ground installed
//!   through the D13 flow;
//! - `wan`: the same at 50 ms each way, 5 ms jitter, 2% loss in bursts,
//!   with a third client joining after 5 s (a keyframe, then deltas) whose
//!   slow plan makes it a late loader at the install;
//! - `soak`: `soak_minutes` at 200 ms round trip, 10% loss in bursts, 2%
//!   duplication, 5% reordering;
//! - `hostile`: the lan session with 2% of datagrams corrupted both ways;
//! - `reconnect`: the lan session with the second client's link cut for
//!   6 s: it times out, joins again by itself and, from a new address once
//!   the link is back, takes its seat back with its token (the host kept
//!   it after its own timeout);
//! - `restart`: the same cut for 2 s, the client's process starting again
//!   from a new address with the old token before the host's timeout: the
//!   host ends the old connection and hands its seat over;
//! - `save_copy`: the wan's link, the host saving at 3 s, an install
//!   behind the copy (I30 P17);
//! - `hold`: the lan session, the host holding its world 6 s (past the
//!   link's timeout): every client is held and starts again;
//! - `reload`: the lan session, the host keeping its world at 4 s in the
//!   proving ground and reloading it in place at 7 s (the party's fall,
//!   I30 P13): every client takes it whole (a keyframe) and plays on.

use sim::TickRate;

use super::client::Cause;
use super::client::RebaseOn;
use super::clock::Margins;
use super::probes::{self, Comeback, Cut, Hitches, Report, Rewinds, Scenario, Setup};
use crate::codec::{CLIENT_RING, SERVER_RING};
use crate::handshake::Refusal;
use crate::netem::Conditions;

/// Clocks must settle within this after each start (µs).
pub const CONVERGE_US: u64 = 2_000_000;
/// The longest an install may reach a player after the host's, with a
/// save copy going out ahead of it: one part (64 KiB, a quarter second at
/// the link's budget), round trips and a retransmit under loss.
pub const INSTALL_BEHIND_COPY_US: u64 = 800_000;

/// What a session's checks expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// A clean link: no own-command mismatches either.
    Clean,
    /// Loss, jitter, joins and installs: everything exact, mismatches
    /// reported and explained.
    Lossy,
    /// Corrupted datagrams: nothing panics, the host steps on, nothing
    /// wrong is taken; clients may be refused.
    Hostile,
}

/// A session to play: its shape, its world and what its checks expect.
pub struct Session {
    pub setup: Setup,
    pub scenario: Box<dyn Scenario + Send>,
    pub expect: Expect,
}

impl Session {
    /// Plays it (`probes::run`).
    pub fn run(&mut self) -> Report {
        probes::run(self.scenario.as_mut(), &self.setup)
    }
}

/// The sessions on the determinism world (see the module docs), the soak
/// `soak_minutes` long.
pub fn determinism(soak_minutes: u64) -> Vec<Session> {
    let lan = Setup {
        name: "lan".into(),
        clients: probes::joiners(0, 100_000),
        conditions: probes::conditions(1, 0, 0, 1),
        seconds: 12,
        every: 2,
        seed: 1,
        load_wait_us: 5_000_000,
        host_plan_us: 150_000,
        corrupt_ppm: 0,
        cut: None,
        hold: None,
        host_frame_us: 0,
        hitches: Hitches::default(),
        margins: Margins::default(),
        rewinds: Rewinds::default(),
        kill_threads_at_us: None,
    };
    let wan = Setup {
        name: "wan".into(),
        clients: probes::joiners(5_000_000, 250_000),
        conditions: probes::conditions(50, 5, 2, 3),
        seconds: 20,
        seed: 2,
        load_wait_us: 800_000,
        ..lan.clone()
    };
    let soak = Setup {
        name: "soak".into(),
        clients: probes::joiners(0, 400_000),
        conditions: Conditions {
            dup_ppm: 20_000,
            reorder_ppm: 50_000,
            ..probes::conditions(100, 10, 10, 3)
        },
        seconds: soak_minutes * 60,
        seed: 3,
        load_wait_us: 30_000_000,
        ..lan.clone()
    };
    let hostile = Setup {
        name: "hostile".into(),
        seed: 6,
        corrupt_ppm: 20_000,
        ..lan.clone()
    };
    let reconnect = Setup {
        name: "reconnect".into(),
        seconds: 20,
        seed: 7,
        cut: Some(Cut {
            from_us: 4_000_000,
            until_us: 10_000_000,
            comeback: Comeback::Rejoins,
        }),
        ..lan.clone()
    };
    let restart = Setup {
        name: "restart".into(),
        seed: 11,
        cut: Some(Cut {
            from_us: 4_000_000,
            until_us: 6_000_000,
            comeback: Comeback::Restarts,
        }),
        ..reconnect.clone()
    };
    // The host saves at 3 s and installs 0.2 s later, behind the copy
    // (I30 P17).
    let save_copy = Setup {
        name: "save_copy".into(),
        conditions: probes::conditions(50, 5, 2, 3),
        seed: 8,
        ..lan.clone()
    };
    // Held 6 s, past `TIMEOUT_US` (5 s): the keepalives go on.
    let hold = Setup {
        name: "hold".into(),
        seconds: 16,
        seed: 10,
        hold: Some((4_000_000, 10_000_000)),
        ..lan.clone()
    };
    let reload = Setup {
        name: "reload".into(),
        seed: 9,
        ..lan.clone()
    };
    // Rewinds from every snapshot, as before issue 229: the same checks.
    let every = Setup {
        name: "lan_every_snapshot".into(),
        seed: 12,
        rewinds: Rewinds {
            every_us: 0,
            ..Rewinds::default()
        },
        ..lan.clone()
    };
    // Rewinds on a thread, as the game makes them: each lands whenever the
    // thread is done in wall time, many frames late in virtual time, with
    // the ticks made meanwhile stepped on it. Its checks are the same; the
    // session is not the same twice.
    let threaded = Setup {
        name: "wan_rewinds_on_a_thread".into(),
        seed: 13,
        rewinds: Rewinds {
            on: RebaseOn::Thread,
            ..Rewinds::default()
        },
        ..wan.clone()
    };
    // A hold (6 s, past the link's timeout) with the rewinds on a thread:
    // one under way when the host holds is dropped, and prediction starts
    // again from the held snapshot at the resume.
    let hold_threaded = Setup {
        name: "hold_rewinds_on_a_thread".into(),
        seed: 15,
        rewinds: threaded.rewinds,
        ..hold.clone()
    };
    // The rewind and step threads die 5 s in, as a panic in a step would:
    // each client goes on in the caller, its world made again from the
    // newest snapshot, every check as on a clean link.
    let threads_die = Setup {
        name: "lan_rewind_threads_die".into(),
        seed: 16,
        rewinds: threaded.rewinds,
        kill_threads_at_us: Some(5_000_000),
        ..lan.clone()
    };
    let threaded_clean = Setup {
        name: "lan_rewinds_on_a_thread".into(),
        seed: 14,
        rewinds: threaded.rewinds,
        ..lan.clone()
    };
    let hz = u64::from(TickRate::default().hz());
    let soak_install = soak.seconds * hz / 3;
    let session = |setup: Setup, install_at: u64, save: Option<u64>, expect: Expect| Session {
        setup,
        scenario: Box::new(probes::Determinism::new(install_at).with_save(save)),
        expect,
    };
    let reloaded = Session {
        setup: reload,
        scenario: Box::new(probes::Determinism::new(2 * hz).with_reload(4 * hz, 7 * hz)),
        expect: Expect::Clean,
    };
    vec![
        session(lan, 6 * hz, None, Expect::Clean),
        session(wan, 8 * hz, None, Expect::Lossy),
        session(soak, soak_install, None, Expect::Lossy),
        session(hostile, 6 * hz, None, Expect::Hostile),
        session(reconnect, 14 * hz, None, Expect::Clean),
        session(restart, 14 * hz, None, Expect::Clean),
        session(save_copy, 3 * hz + hz / 5, Some(3 * hz), Expect::Lossy),
        session(hold, 12 * hz, None, Expect::Clean),
        reloaded,
        session(every, 6 * hz, None, Expect::Clean),
        session(threaded, 8 * hz, None, Expect::Lossy),
        session(threaded_clean, 6 * hz, None, Expect::Clean),
        session(hold_threaded, 12 * hz, None, Expect::Clean),
        session(threads_die, 6 * hz, None, Expect::Clean),
    ]
}

/// One check of a session. One that does not bind (a hostile client's
/// own, a lossy link's prediction) is a note when it fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub binding: bool,
    pub pass: bool,
    pub detail: String,
}

impl Check {
    /// Failed, and binding.
    pub fn failed(&self) -> bool {
        self.binding && !self.pass
    }
}

/// The checks of a session's `report`, in order.
pub fn checks(report: &Report, expect: Expect) -> Vec<Check> {
    let mut checks = Vec::new();
    let mut check = |name: &str, binding: bool, pass: bool, detail: String| {
        checks.push(Check {
            name: name.to_string(),
            binding,
            pass,
            detail,
        });
    };
    // In the hostile session the first client is the hostile one: its own
    // checks are notes; the host's and everyone else's bind.
    let binding = |index: usize| expect != Expect::Hostile || index != 0;
    if expect == Expect::Hostile {
        check(
            "hostile client",
            true,
            report.corrupted > 0 && report.ticks > 0,
            format!(
                "{} of client 0's datagrams corrupted on their way to the host: no panic, the host stepped {} ticks",
                report.corrupted, report.ticks
            ),
        );
    }
    check(
        "replay",
        true,
        report.replayed(),
        format!(
            "the host's recording (what it applied) replays to its end {}: {}",
            report.end.short(),
            match &report.replay {
                Ok(hash) => hash.short(),
                Err(error) => error.clone(),
            }
        ),
    );
    let exact: Vec<String> = report
        .clients
        .iter()
        .map(|c| format!("{}/{}", c.checked - c.wrong, c.checked))
        .collect();
    check(
        "exact",
        true,
        report.exact(),
        format!(
            "every snapshot a client rebuilt hashes as the host's ({}), check failures {}",
            exact.join(", "),
            report
                .clients
                .iter()
                .map(|c| c.stats.check_failures)
                .sum::<u64>()
        ),
    );
    for index in 0..report.clients.len() {
        client_checks(&mut check, report, index, binding(index), expect);
    }
    // The host's saves reach every player byte for byte, and an install
    // after one waits for one part at most (I30 P17).
    if !report.saves.is_empty() {
        for (index, client) in report.clients.iter().enumerate() {
            let same = client
                .copies
                .iter()
                .zip(&report.saves)
                .filter(|(a, b)| a == b)
                .count();
            check(
                &format!("client {index} save copies"),
                binding(index),
                client.copies == report.saves,
                format!(
                    "{same} of {} copies ({} bytes), byte for byte",
                    report.saves.len(),
                    report.saves.iter().map(Vec::len).sum::<usize>()
                ),
            );
            check(
                &format!("client {index} install not held by a copy"),
                binding(index),
                client.install_delay_max_us <= INSTALL_BEHIND_COPY_US,
                format!(
                    "Installing reached it {:.0} ms after the host's (one part at most: {} ms)",
                    client.install_delay_max_us as f64 / 1e3,
                    INSTALL_BEHIND_COPY_US / 1000
                ),
            );
        }
    }
    for (index, client) in report.clients.iter().enumerate() {
        if client.holds > 0 {
            check(
                &format!("client {index} held and started again"),
                binding(index),
                client.ended.is_none() && client.starts.len() as u64 > client.holds,
                format!(
                    "held {} time(s), {} start(s)",
                    client.holds,
                    client.starts.len()
                ),
            );
        }
    }
    let bounded = report.server_ring_max <= SERVER_RING
        && report.clients.iter().all(|c| {
            c.memory_max.ring <= CLIENT_RING
                && c.memory_max.reassembly <= 4
                && c.memory_max.own <= 128
                && c.memory_max.history <= 128
                && c.memory_max.seat_commands <= 128
        });
    check(
        "memory",
        true,
        bounded,
        format!(
            "bounded: host ring max {} (limit {}), applied {}, client rings max {:?}",
            report.server_ring_max,
            SERVER_RING,
            report.server.applied,
            report
                .clients
                .iter()
                .map(|c| c.memory_max.ring)
                .collect::<Vec<_>>()
        ),
    );
    checks
}

/// Whether the host ended a cut client's old connection as its comeback
/// should have: timed out (its seat kept for the token), or ended when the
/// client joined again before that.
fn old_ended_as(comeback: Option<Comeback>, why: &Option<Refusal>) -> bool {
    matches!(
        (comeback, why),
        (Some(Comeback::Rejoins), Some(Refusal::Timeout))
            | (Some(Comeback::Restarts), Some(Refusal::Kicked(_)))
    )
}

/// Client `index`'s checks. Its prediction binds on a clean link only.
fn client_checks(
    check: &mut impl FnMut(&str, bool, bool, String),
    report: &Report,
    index: usize,
    binding: bool,
    expect: Expect,
) {
    let client = &report.clients[index];
    let stats = &client.stats;
    check(
        &format!("client {index} connected"),
        binding,
        client.ended.is_none(),
        client
            .ended
            .as_ref()
            .map_or("played to the end".to_string(), ToString::to_string),
    );
    if let Some((before, after)) = client.reconnected {
        check(
            &format!("client {index} reconnects"),
            binding,
            before.is_some() && before == after && old_ended_as(report.comeback, &client.old_left),
            format!(
                "cut in seat {}, came back from a new address with its token into seat {}; the host ended the old connection: {}",
                before.map_or("-".into(), |s| s.0.to_string()),
                after.map_or("-".into(), |s| s.0.to_string()),
                client
                    .old_left
                    .as_ref()
                    .map_or("not at all".into(), ToString::to_string)
            ),
        );
    }
    if report.installs > 0 && client.joined_at_us < 1_000_000 {
        check(
            &format!("client {index} installs"),
            binding,
            client.installs + report.reloads == report.installs
                && stats.installs_differed == report.reloads,
            format!(
                "committed {} of the host's {} install(s), each check_after equal to the announcement ({} differed: {} reload(s), taken whole); a late loader {} time(s)",
                client.installs,
                report.installs,
                stats.installs_differed,
                report.reloads,
                client.late_loads
            ),
        );
    }
    if client.joined_at_us >= 1_000_000 {
        check(
            &format!("client {index} joins mid-session"),
            binding,
            stats.keyframes >= 1 && stats.rebuilt > stats.keyframes,
            format!(
                "joined at {:.1} s: {} keyframe(s), then {} deltas",
                client.joined_at_us as f64 / 1e6,
                stats.keyframes,
                stats.rebuilt - stats.keyframes
            ),
        );
    }
    let converged: Vec<String> = client
        .starts
        .iter()
        .map(|start| match start.converged_us {
            Some(us) => format!("{:.0} ms", us as f64 / 1e3),
            None => format!("not in its {:.0} ms", start.lasted_us as f64 / 1e3),
        })
        .collect();
    check(
        &format!("client {index} clocks"),
        binding,
        client
            .starts
            .iter()
            .any(|start| start.lasted_us >= CONVERGE_US)
            && client
                .starts
                .iter()
                .all(|start| start.converged_within(CONVERGE_US)),
        format!(
            "lead and remote delay within half a tick of their targets, per start (≤ 2 s; a start an install stops sooner cannot tell): {} (worst after: lead {:.2}, delay {:.2} ticks); now lead {} ticks, delay {} ticks, jitter p95 {} µs",
            converged.join(", "),
            client.worst_after.0,
            client.worst_after.1,
            client
                .clock
                .map_or("-".into(), |c| format!("{:.2}", c.lead)),
            client
                .clock
                .map_or("-".into(), |c| format!("{:.2}", c.delay)),
            client.clock.map_or(0, |c| c.jitter_us),
        ),
    );
    check(
        &format!("client {index} rewinds"),
        binding,
        stats.rewinds_beside > 0 && stats.rewinds_failed == 0,
        format!(
            "{} rewinds ({} made while frames went on; mean {:.1} ticks, deepest {}), {} for a world since replaced and dropped, {} that did not restore, {} lost with their thread",
            stats.rewinds,
            stats.rewinds_beside,
            stats.rewind_ticks as f64 / stats.rewinds.max(1) as f64,
            stats.rewind_max,
            stats.rewinds_dropped,
            stats.rewinds_failed,
            stats.rewinds_lost
        ),
    );
    check(
        &format!("client {index} own time goes forward"),
        binding,
        client.view_backs == 0,
        format!(
            "{} frames showed its own seat earlier than the frame before (the most by {:.1} ms)",
            client.view_backs, client.view_back_max_ms
        ),
    );
    if report.threads_killed {
        check(
            &format!("client {index} threads died"),
            binding,
            client.threads_lost,
            format!(
                "its rewind and step threads died {:.1} s in and it went on in the caller: lost {}",
                report.threads_killed_at_us as f64 / 1e6,
                client.threads_lost
            ),
        );
    }
    let own =
        stats.by_cause[Cause::OwnMissed as usize] + stats.by_cause[Cause::OwnDiffers as usize];
    let causes: Vec<String> = Cause::ALL
        .iter()
        .map(|cause| format!("{} {}", cause.name(), stats.by_cause[*cause as usize]))
        .collect();
    let first: Vec<String> = stats
        .mismatch_log
        .iter()
        .take(6)
        .map(|m| format!("{}:{}", m.tick, m.cause.name()))
        .collect();
    check(
        &format!("client {index} prediction"),
        binding && expect == Expect::Clean,
        own == 0 && stats.compared > 0,
        format!(
            "{} of {} predicted avatars differ from the host's ({}); first: {}",
            stats.mismatches,
            stats.compared,
            causes.join(", "),
            if first.is_empty() {
                "-".into()
            } else {
                first.join(", ")
            }
        ),
    );
}
