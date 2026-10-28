//! `kjk net margins`: what each of the clocks' margins buys. Sessions in
//! virtual time (`net::session::probes`) on the determinism world, one
//! player and a host that steps in its frames (`FRAMES`: both at 60 Hz as
//! `cargo xtask duo` runs them, a loaded machine, a 144 Hz player or host),
//! at each round trip, jitter and loss, with each set of margins (the
//! design's, and each part left out): the lead and the remote delay
//! they cost, and what they keep away (starvations, the host's late and
//! substituted commands, own-avatar corrections). Prints a markdown table
//! and writes `facts.json`.

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use net::session::Cause;
use net::session::clock::Margins;
use net::session::probes::{self, Hitches, Joiner, Report, Setup};
use serde_json::json;
use sim::TickRate;

/// 60 and 144 Hz frames, µs.
const HZ60: u64 = 16_667;
const HZ144: u64 = 6_944;

/// The conditions swept, one way: jitter (ms), loss (percent, burst).
const JITTERS: [u64; 3] = [0, 5, 20];
const LOSSES: [(u64, u64); 3] = [(0, 1), (2, 1), (2, 3)];

/// How the two processes draw: the player's frame and the host's (µs), and
/// how often frames run long.
struct Frames {
    name: &'static str,
    player_us: u64,
    host_us: u64,
    hitches: Hitches,
}

const EVEN: Hitches = Hitches { ppm: 0, max_us: 0 };

/// Both at 60 Hz, as `cargo xtask duo` runs; the same on a loaded machine
/// (one frame in 20 up to 60 ms longer, the host's and the player's); a
/// player at 144 Hz; a host at 144 Hz.
const FRAMES: [Frames; 4] = [
    Frames {
        name: "60/60",
        player_us: HZ60,
        host_us: HZ60,
        hitches: EVEN,
    },
    Frames {
        name: "60/60 loaded",
        player_us: HZ60,
        host_us: HZ60,
        hitches: Hitches {
            ppm: 50_000,
            max_us: 60_000,
        },
    },
    Frames {
        name: "144/60",
        player_us: HZ144,
        host_us: HZ60,
        hitches: EVEN,
    },
    Frames {
        name: "60/144",
        player_us: HZ60,
        host_us: HZ144,
        hitches: EVEN,
    },
];

/// The margins compared: the design's, and the design with each part left
/// out (or loosened).
fn settings() -> Vec<(&'static str, Margins)> {
    let design = Margins::DESIGN;
    vec![
        ("design", design),
        (
            "lead: no 1-tick floor",
            Margins {
                slack_floor_16ths: 0,
                ..design
            },
        ),
        (
            "lead: no half tick",
            Margins {
                slack_extra_16ths: 0,
                ..design
            },
        ),
        (
            "lead: no jitter",
            Margins {
                slack_jitter: false,
                ..design
            },
        ),
        (
            "lead: no loss margin",
            Margins {
                loss_frames: 0,
                ..design
            },
        ),
        (
            "D: no 4 ms",
            Margins {
                delay_extra_us: 0,
                ..design
            },
        ),
        (
            "D: no starvation step",
            Margins {
                starve_max: 0,
                ..design
            },
        ),
        (
            "D: step decays in 1 s",
            Margins {
                starve_decay_us: 1_000_000,
                ..design
            },
        ),
        (
            "D: p90",
            Margins {
                delay_percentile: 90,
                ..design
            },
        ),
    ]
}

/// A link and the frames on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Condition {
    rtt_ms: u64,
    jitter_ms: u64,
    loss: (u64, u64),
    frames: usize,
}

impl Condition {
    fn setup(&self, seconds: u64, margins: Margins) -> Setup {
        let frames = &FRAMES[self.frames];
        Setup {
            name: format!(
                "{}/{}/{}%x{}/{}",
                self.rtt_ms, self.jitter_ms, self.loss.0, self.loss.1, frames.name
            ),
            clients: vec![Joiner {
                at_us: 0,
                frame_us: frames.player_us,
                plan_us: 100_000,
            }],
            conditions: probes::conditions(
                self.rtt_ms / 2,
                self.jitter_ms,
                self.loss.0,
                self.loss.1,
            ),
            seconds,
            every: 2,
            seed: self.rtt_ms * 1000 + self.jitter_ms * 10 + self.loss.0 + self.loss.1,
            load_wait_us: 5_000_000,
            host_plan_us: 0,
            corrupt_ppm: 0,
            cut: None,
            hold: None,
            host_frame_us: frames.host_us,
            hitches: frames.hitches,
            margins,
        }
    }
}

/// One session's measures.
struct Row {
    setting: usize,
    condition: Condition,
    report: Report,
}

pub fn margins(seconds: u64, rtts: &[u64], only: Option<&str>) -> ExitCode {
    let started = Instant::now();
    let hz = u64::from(TickRate::default().hz());
    let settings = settings();
    let mut jobs = Vec::new();
    for (index, (name, margins)) in settings.iter().enumerate() {
        if only.is_some_and(|only| !name.contains(only)) {
            continue;
        }
        for &rtt_ms in rtts {
            for jitter_ms in JITTERS {
                for loss in LOSSES {
                    for frames in 0..FRAMES.len() {
                        let condition = Condition {
                            rtt_ms,
                            jitter_ms,
                            loss,
                            frames,
                        };
                        jobs.push((index, *margins, condition));
                    }
                }
            }
        }
    }
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let next = AtomicUsize::new(0);
    let mut rows: Vec<Row> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut rows = Vec::new();
                    while let Some(&(setting, margins, condition)) =
                        jobs.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        // No install: the clocks run the whole session.
                        let mut scenario = probes::Determinism::new(seconds * hz * 2);
                        let report = probes::run(&mut scenario, &condition.setup(seconds, margins));
                        rows.push(Row {
                            setting,
                            condition,
                            report,
                        });
                    }
                    rows
                })
            })
            .collect();
        workers
            .into_iter()
            .filter_map(|worker| worker.join().ok())
            .flatten()
            .collect()
    });
    rows.sort_by_key(|row| (row.condition, row.setting));
    outln!(
        "{} sessions of {seconds} virtual s in {:.1} s; frames: the player's / the host's rate; lead and D are means (ms); per minute: starvations (frames held), the host's late / substituted commands, own-avatar mismatches / corrections\n",
        rows.len(),
        started.elapsed().as_secs_f64()
    );
    outln!(
        "| margins | RTT | jitter | loss | frames | lead | D | lag − RTT | starved (frames) | late / subst | mismatch / corr |\n|---|---|---|---|---|---|---|---|---|---|---|"
    );
    let tick_ms = 1000.0 / hz as f64;
    let mut facts = Vec::new();
    for row in &rows {
        let Some(client) = row.report.clients.first() else {
            continue;
        };
        let (name, condition) = (settings[row.setting].0, row.condition);
        let stats = &client.stats;
        let per_min = |count: u64| count as f64 * 60.0 / row.report.seconds.max(1) as f64;
        let lead = client.mean_after.0 * tick_ms;
        let delay = client.mean_after.1 * tick_ms;
        let d = delay - client.link.rtt_min_us as f64 / 2e3;
        let own =
            stats.by_cause[Cause::OwnMissed as usize] + stats.by_cause[Cause::OwnDiffers as usize];
        outln!(
            "| {name} | {} | {} | {}% ×{} | {} | {lead:.0} | {d:.0} | {:.0} | {:.1} ({:.0}) | {:.1} / {:.1} | {:.1} / {:.1} |",
            condition.rtt_ms,
            condition.jitter_ms,
            condition.loss.0,
            condition.loss.1,
            FRAMES[condition.frames].name,
            lead + delay - condition.rtt_ms as f64,
            per_min(stats.starvations),
            per_min(stats.starved_frames),
            per_min(stats.host_late),
            per_min(stats.host_substituted),
            per_min(own),
            per_min(stats.corrections),
        );
        facts.push(json!({
            "margins": name,
            "rtt_ms": condition.rtt_ms,
            "jitter_ms": condition.jitter_ms,
            "loss_percent": condition.loss.0,
            "loss_burst": condition.loss.1,
            "frames": FRAMES[condition.frames].name,
            "seconds": row.report.seconds,
            "lead_ms": lead,
            "delay_ms": delay,
            "d_ms": d,
            "rtt_min_ms": client.link.rtt_min_us as f64 / 1e3,
            "starvations": stats.starvations,
            "starved_frames": stats.starved_frames,
            "host_late": stats.host_late,
            "host_substituted": stats.host_substituted,
            "skipped": stats.skipped,
            "own_mismatches": own,
            "corrections": stats.corrections,
            "exact": row.report.exact(),
        }));
    }
    crate::runtime::write_facts(json!({ "verb": "net margins", "sessions": facts }));
    let inexact = rows.iter().filter(|row| !row.report.exact()).count();
    if inexact == 0 {
        ExitCode::SUCCESS
    } else {
        outln!("net margins: {inexact} sessions rebuilt a snapshot unlike the host's");
        ExitCode::FAILURE
    }
}
