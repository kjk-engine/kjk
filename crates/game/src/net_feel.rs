//! D16's net feel. As a session runs, every `SAMPLE` each side writes down
//! what its network does: round trip, jitter, loss, bytes each way per
//! second, the snapshot interval, and the host's input slack or the
//! player's lead and remote delay. Each sample is a Perfetto counter
//! (`perf`, with `KJK_PERF=1`) and a row of `net.json`'s `feel`. `kjk net
//! feel` turns a host's and a player's `net.json` into `net-feel.md`
//! (mean, p95 and max of each, the run's counts, and input to photon);
//! `cargo xtask duo` writes it for every run.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// How often a side samples its network.
pub const SAMPLE: Duration = Duration::from_millis(250);
/// A tick, ms (125 Hz).
pub const TICK_MS: f64 = 8.0;

/// One side's samples, and its frames' times.
pub struct Feel {
    started: Instant,
    last_sample: Option<Instant>,
    last_frame: Option<Instant>,
    frames: u64,
    frame_ms: f64,
    /// Each track's last sample: when (s) and its running totals, to make
    /// rates of.
    totals: BTreeMap<String, (f64, Vec<f64>)>,
    /// The host's: each player's input slack summed (µs) and its commands
    /// at the last sample, to make the sample's mean of.
    slack: BTreeMap<u8, (i64, u64)>,
    samples: Vec<Value>,
}

impl Default for Feel {
    fn default() -> Feel {
        Feel {
            started: Instant::now(),
            last_sample: None,
            last_frame: None,
            frames: 0,
            frame_ms: 0.0,
            totals: BTreeMap::new(),
            slack: BTreeMap::new(),
            samples: Vec::new(),
        }
    }
}

impl Feel {
    /// A frame passed: whether a sample is due.
    pub fn frame(&mut self) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            self.frames += 1;
            self.frame_ms += now.duration_since(last).as_secs_f64() * 1000.0;
        }
        self.last_frame = Some(now);
        let due = self
            .last_sample
            .is_none_or(|last| now.duration_since(last) >= SAMPLE);
        if due {
            self.last_sample = Some(now);
        }
        due
    }

    /// A sample of `track`: `gauges` as they are, `totals` (running
    /// counts) as rates per second since the track's last sample (none on
    /// its first).
    pub fn sample(&mut self, track: &str, gauges: &[(&str, f64)], totals: &[(&str, f64)]) {
        let now = self.started.elapsed().as_secs_f64();
        let current: Vec<f64> = totals.iter().map(|(_, value)| *value).collect();
        let mut values: Vec<(&str, f64)> = gauges.to_vec();
        if let Some((at, before)) = self.totals.insert(track.to_string(), (now, current)) {
            let seconds = (now - at).max(1e-3);
            for ((name, value), before) in totals.iter().zip(before) {
                values.push((name, (value - before) / seconds));
            }
        }
        crate::perf::counter(track, &values);
        let mut row = serde_json::Map::new();
        row.insert("t".into(), json!(now));
        row.insert("track".into(), json!(track));
        for (name, value) in values {
            row.insert(name.to_string(), json!(value));
        }
        self.samples.push(Value::Object(row));
    }

    /// For `net.json`: the frames' mean and every sample.
    pub fn report(&self) -> Value {
        json!({
            "sample_ms": SAMPLE.as_millis() as u64,
            "frame_ms_mean": self.frame_ms / self.frames.max(1) as f64,
            "samples": self.samples,
        })
    }
}

/// The host's samples: one track per player over the network.
pub fn sample_host(feel: &mut Feel, server: &net::session::ServerStats) {
    for c in server
        .connections
        .iter()
        .filter(|c| matches!(c.peer, net::Peer::Addr(_)))
    {
        // The mean slack of the commands since the last sample.
        let now = (c.stats.slack_sum_us, c.stats.commands);
        let (sum, count) = feel.slack.insert(c.seat.0, now).unwrap_or((0, 0));
        let commands = now.1.saturating_sub(count);
        let mut gauges = vec![
            ("rtt_ms", c.link.rtt_us as f64 / 1000.0),
            ("jitter_ms", c.link.jitter_us as f64 / 1000.0),
            ("loss_percent", c.link.loss_ppm as f64 / 10_000.0),
            ("interval_ticks", f64::from(c.every)),
        ];
        if commands > 0 {
            gauges.push(("slack_ms", (now.0 - sum) as f64 / commands as f64 / 1000.0));
        }
        let totals = [
            ("kb_down_s", c.link.bytes_sent as f64 / 1000.0),
            ("kb_up_s", c.link.bytes_received as f64 / 1000.0),
        ];
        feel.sample(&format!("net seat {}", c.seat.0), &gauges, &totals);
    }
}

/// A player's sample.
pub fn sample_player(feel: &mut Feel, client: &net::session::Client) {
    let (Some(link), Some(clock)) = (client.link_stats(), client.clock()) else {
        return;
    };
    let gauges = [
        ("rtt_ms", link.rtt_us as f64 / 1000.0),
        ("jitter_ms", link.jitter_us as f64 / 1000.0),
        ("loss_percent", link.loss_ppm as f64 / 10_000.0),
        ("lead_ms", clock.lead * TICK_MS),
        ("delay_ms", clock.delay * TICK_MS),
        ("target_slack_ms", clock.slack_us as f64 / 1000.0),
        ("d_ms", clock.d_us as f64 / 1000.0),
        ("jitter_p95_ms", clock.jitter_us as f64 / 1000.0),
        ("loss_margin_frames", clock.loss_frames as f64),
        ("frame_counted_ms", clock.frame_us as f64 / 1000.0),
    ];
    let stats = client.stats();
    let totals = [
        ("kb_down_s", link.bytes_received as f64 / 1000.0),
        ("kb_up_s", link.bytes_sent as f64 / 1000.0),
        ("snapshots_s", stats.rebuilt as f64),
        ("skipped_s", stats.skipped as f64),
        ("substituted_s", stats.host_substituted as f64),
    ];
    feel.sample("net", &gauges, &totals);
}
