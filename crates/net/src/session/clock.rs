//! A client's two timelines (design section 7, D9), both positions on the
//! host's tick timeline, where tick `t` is stepped at `time_us(t)`:
//!
//! - **predicted time** runs ahead of the host: the client makes the
//!   command for tick `t` when it passes `time_us(t)`, so the command
//!   arrives a little before the host steps it;
//! - **remote time** runs behind: remote entities are drawn between the
//!   two authority snapshots around it.
//!
//! **Where the host is.** Every snapshot frame names its tick and how long
//! after the step it left (`since`). On arrival at local time `a`,
//! `a − time_us(tick) − since` is the one-way delay plus the clocks'
//! offset. The least of
//! those over the last `WINDOW_US` says most about the offset (the least
//! delayed packets); with half the smallest round trip added back it gives
//! the host's time now. Jitter is the spread of the samples above that
//! least one (95th percentile).
//!
//! **Steering the lead by what the host saw.** The host reports the least
//! slack (how long before the step of its tick a command arrived, measured
//! at the step; negative for one that came after) since its last report,
//! and whose command that was. The client
//! remembers where its predicted clock stood against its own clock when
//! it made that command, so each report says where the clock *should*
//! have stood: `offset_then − (slack − target)`. Those are smoothed
//! (1/16 per report) into the offset the clock steers to. The loop
//! delay (a round trip before a change shows in the reports) does not
//! matter: each report is compared with the offset of its own command,
//! not with today's. The target slack is `max(1 tick, p95 jitter) + ½
//! tick`.
//!
//! **Loss.** Every packet carries all unacked commands, so a lost packet
//! only delays its commands to the next frame's packet; a burst of them
//! outlasts the slack above. When the host reports a command this side
//! made late or missing, the target slack grows by one frame (at most
//! every 0.5 s, to three frames); it shrinks by one after 10 s without a
//! miss. A tick this side skipped (a long frame, a jump) is no miss: it had
//! no command to lose.
//!
//! **Moving towards a target** by time dilation: a clock runs at `1 + d`
//! of real time, `d = 3% per tick of error`, within ±5%, not at all
//! within a quarter tick; beyond 8 ticks of error it jumps. Remote time
//! steers to `D` behind the host's snapshots as the least-delayed arrive
//! (so the view lags the host by a one-way trip and `D`, C18's view lag),
//! `D = interval + p95 jitter + 4 ms` plus one
//! interval per starvation (when remote time passes the newest snapshot),
//! which decays after 5 s without one.

use std::collections::VecDeque;

use sim::TickRate;

use crate::NetTime;

/// How far back the delay samples go.
pub const WINDOW_US: u64 = 2_000_000;
/// Dilation per tick of error, parts per million.
const GAIN_PPM: i64 = 30_000;
/// The most dilation, parts per million.
const MAX_DILATION_PPM: i64 = 50_000;
/// Error in ticks beyond which a clock jumps.
pub const RESYNC_TICKS: i64 = 8;
/// The loss margin grows at most this often, and shrinks after this long
/// without a miss.
const MARGIN_EVERY_US: u64 = 500_000;
const MARGIN_DECAY_US: u64 = 10_000_000;

/// The host's tick timeline: when each tick is stepped, in µs.
pub fn time_us(rate: TickRate, tick: u64) -> i64 {
    rate.time_ms(tick) as i64 * 1000
}

/// The tick whose step is the last at or before `time` (µs on the tick
/// timeline).
pub fn tick_at(rate: TickRate, time: i64) -> u64 {
    if time <= 0 {
        return 0;
    }
    // time_ms(t) = t * 1000 / hz, so t = floor(ms * hz / 1000) or one more.
    let ms = (time / 1000) as u64;
    let mut tick = ms * u64::from(rate.hz()) / 1000;
    while time_us(rate, tick + 1) <= time {
        tick += 1;
    }
    tick
}

/// The margins the clocks keep over the bare trip (`kjk net margins`
/// measures what each buys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Margins {
    /// The target slack: `max(floor, p95 jitter) + extra`, in sixteenths of
    /// a tick (`slack_jitter: false` leaves the jitter out).
    pub slack_floor_16ths: i64,
    pub slack_extra_16ths: i64,
    pub slack_jitter: bool,
    /// The most frames the loss margin adds to the target slack.
    pub loss_frames: i64,
    /// The remote delay covers this percentile of the delay spread, plus
    /// `delay_extra_us`.
    pub delay_percentile: u8,
    pub delay_extra_us: i64,
    /// The most snapshot intervals starvations add to the remote delay,
    /// and how long each lasts without another starvation.
    pub starve_max: i64,
    pub starve_decay_us: u64,
}

impl Margins {
    /// The design's margins (I15 sections 7 and 8).
    pub const DESIGN: Margins = Margins {
        slack_floor_16ths: 16,
        slack_extra_16ths: 8,
        slack_jitter: true,
        loss_frames: 3,
        delay_percentile: 95,
        delay_extra_us: 4_000,
        starve_max: 8,
        starve_decay_us: 5_000_000,
    };
}

impl Default for Margins {
    fn default() -> Margins {
        Margins::DESIGN
    }
}

/// The delay samples of the last `WINDOW_US`: the least, and the spread
/// above it.
#[derive(Debug, Clone)]
pub struct Delays {
    samples: VecDeque<(NetTime, i64)>,
    least: Option<i64>,
    p95: i64,
    /// The spread at the remote delay's percentile.
    spread: i64,
    percentile: u8,
    scratch: Vec<i64>,
}

impl Default for Delays {
    fn default() -> Delays {
        Delays::new(95)
    }
}

impl Delays {
    /// Samples whose spread the remote delay covers to `percentile`.
    pub fn new(percentile: u8) -> Delays {
        Delays {
            samples: VecDeque::new(),
            least: None,
            p95: 0,
            spread: 0,
            percentile,
            scratch: Vec::new(),
        }
    }

    /// A sample at local time `at`: arrival minus the host's time of
    /// sending, µs.
    pub fn sample(&mut self, at: NetTime, offset: i64) {
        self.samples.push_back((at, offset));
        while self
            .samples
            .front()
            .is_some_and(|(then, _)| at.since(*then) > WINDOW_US)
        {
            self.samples.pop_front();
        }
        let least = self.samples.iter().map(|(_, sample)| *sample).min();
        self.least = least;
        let least = least.unwrap_or(offset);
        self.scratch.clear();
        self.scratch
            .extend(self.samples.iter().map(|(_, sample)| sample - least));
        self.p95 = self.percentile_of(95);
        self.spread = self.percentile_of(self.percentile);
    }

    /// The `percent`th percentile of the spread in `scratch`.
    fn percentile_of(&mut self, percent: u8) -> i64 {
        let at = (self.scratch.len() * usize::from(percent) / 100).min(self.scratch.len() - 1);
        *self.scratch.select_nth_unstable(at).1
    }

    /// Forgets every sample (the host's timeline moved: an install).
    pub fn reset(&mut self) {
        self.samples.clear();
        self.least = None;
        self.p95 = 0;
        self.spread = 0;
    }

    /// The least sample, once there is one.
    pub fn least(&self) -> Option<i64> {
        self.least
    }

    /// The 95th percentile of the samples above the least, µs.
    pub fn jitter(&self) -> i64 {
        self.p95
    }

    /// The remote delay's percentile of the samples above the least, µs.
    pub fn spread(&self) -> i64 {
        self.spread
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// What steering does to a clock this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steer {
    /// Run at `1 + ppm / 1e6` of real time.
    Rate(i64),
    /// Jump to the target.
    Jump,
}

/// How to move a clock `error_us` behind its target (negative: ahead).
pub fn steer(error_us: i64, tick_us: i64) -> Steer {
    if error_us.abs() > RESYNC_TICKS * tick_us {
        Steer::Jump
    } else if error_us.abs() * 4 < tick_us {
        Steer::Rate(0)
    } else {
        Steer::Rate((error_us * GAIN_PPM / tick_us).clamp(-MAX_DILATION_PPM, MAX_DILATION_PPM))
    }
}

/// Both clocks, their estimate of the host's time and their targets.
#[derive(Debug, Clone)]
pub struct Clocks {
    pub rate: TickRate,
    pub delays: Delays,
    /// The smallest round trip seen (from the link), µs.
    pub rtt_min_us: i64,
    /// The smoothed round trip, µs.
    pub rtt_us: i64,
    /// The longest recent frame, µs: a frame makes its commands up to
    /// this late.
    pub frame_us: i64,
    /// Predicted time minus local time, µs.
    pub predicted: i64,
    /// Remote time minus local time, µs.
    pub remote: i64,
    /// The offset the predicted clock steers to, from the host's reports
    /// (none yet: from the estimate).
    ideal: Option<i64>,
    /// Frames added to the target slack for loss, and when it last grew
    /// or shrank.
    pub margin: i64,
    margin_at: Option<NetTime>,
    /// Starvation intervals added to the remote delay, and when the last
    /// happened.
    starved: i64,
    starved_at: NetTime,
    /// The host's snapshot interval, ticks.
    pub every: u8,
    /// Dilations applied by the last `advance`, ppm.
    pub predicted_ppm: i64,
    pub remote_ppm: i64,
    /// Jumps taken.
    pub jumps: u64,
    pub margins: Margins,
}

impl Clocks {
    pub fn new(rate: TickRate, every: u8, margins: Margins) -> Clocks {
        Clocks {
            rate,
            delays: Delays::new(margins.delay_percentile),
            rtt_min_us: 0,
            rtt_us: 0,
            frame_us: 0,
            predicted: 0,
            remote: 0,
            ideal: None,
            margin: 0,
            margin_at: None,
            starved: 0,
            starved_at: NetTime(0),
            every: every.max(1),
            predicted_ppm: 0,
            remote_ppm: 0,
            jumps: 0,
            margins,
        }
    }

    pub fn tick_us(&self) -> i64 {
        time_us(self.rate, 1).max(1)
    }

    /// A snapshot of `tick`, sent `since_us` after its step, arrived at `at`.
    pub fn sample(&mut self, at: NetTime, tick: u64, since_us: u64) {
        let sent = time_us(self.rate, tick) + since_us as i64;
        self.delays.sample(at, at.0 as i64 - sent);
    }

    /// Where the host's timeline is at local time `now`, once a sample
    /// came.
    pub fn host_offset(&self) -> Option<i64> {
        let least = self.delays.least()?;
        Some(self.rtt_min_us / 2 - least)
    }

    /// The slack the client wants its commands to arrive with, µs: the
    /// design's `max(1 tick, p95 jitter) + ½ tick` (`Margins`), plus a frame
    /// for each step of the loss margin (a lost packet delays its commands
    /// to the next frame's).
    pub fn target_slack(&self) -> i64 {
        let tick = self.tick_us();
        let margins = &self.margins;
        let jitter = if margins.slack_jitter {
            self.delays.jitter()
        } else {
            0
        };
        (margins.slack_floor_16ths * tick / 16).max(jitter)
            + margins.slack_extra_16ths * tick / 16
            + self.margin * self.frame_us
    }

    /// The host reported commands late or missing at `now`: the margin
    /// grows a frame (at most once per `MARGIN_EVERY_US`, to
    /// `Margins::loss_frames`).
    pub fn missed(&mut self, now: NetTime) {
        if self
            .margin_at
            .is_none_or(|at| now.since(at) >= MARGIN_EVERY_US)
        {
            self.margin = (self.margin + 1).min(self.margins.loss_frames);
            self.margin_at = Some(now);
        }
    }

    /// The remote delay target, µs.
    pub fn target_delay(&self) -> i64 {
        let tick = self.tick_us();
        (i64::from(self.every) + self.starved) * tick
            + self.delays.spread()
            + self.margins.delay_extra_us
    }

    /// The predicted clock's target offset: from the host's reports, or
    /// before any, from the estimate (half the round trip, the slack
    /// wanted, and a frame).
    pub fn predicted_target(&self) -> Option<i64> {
        self.ideal.or_else(|| {
            self.host_offset()
                .map(|host| host + self.rtt_us / 2 + self.target_slack() + self.frame_us)
        })
    }

    /// A frame `elapsed_us` after the last: the longest recent one decays
    /// towards the newest (1/16 a frame).
    pub fn frame(&mut self, elapsed_us: i64) {
        self.frame_us = if elapsed_us >= self.frame_us {
            elapsed_us
        } else {
            self.frame_us - (self.frame_us - elapsed_us) / 16
        };
    }

    /// The remote clock's target offset: `D` behind the host's snapshots
    /// as they arrive (the least-delayed ones), not behind the host's now,
    /// which no snapshot has reached yet.
    pub fn remote_target(&self) -> Option<i64> {
        self.delays
            .least()
            .map(|least| -least - self.target_delay())
    }

    /// The host's report for a command made when the predicted clock stood
    /// `offset_then` ahead of local time: it arrived `slack_us` before its
    /// step (negative: after).
    pub fn report(&mut self, offset_then: i64, slack_us: i64) {
        let wanted = offset_then - (slack_us - self.target_slack());
        // The first report moves on from the estimate, not from nothing.
        let ideal = self
            .ideal
            .or_else(|| self.predicted_target())
            .unwrap_or(wanted);
        self.ideal = Some(ideal + (wanted - ideal) / 16);
    }

    /// Starts both clocks at their targets (a join, a resume).
    pub fn start(&mut self) {
        if let Some(target) = self.predicted_target() {
            self.predicted = target;
        }
        if let Some(target) = self.remote_target() {
            self.remote = target;
        }
    }

    /// After an install: the host's timeline restarts, and the reports of
    /// the old one no longer apply.
    pub fn restart(&mut self) {
        self.delays.reset();
        self.ideal = None;
        self.starved = 0;
    }

    /// Remote time passed the newest snapshot.
    pub fn starve(&mut self, now: NetTime) {
        self.starved = (self.starved + 1).min(self.margins.starve_max);
        self.starved_at = now;
    }

    /// Moves both clocks on by `elapsed_us` of real time, steered.
    pub fn advance(&mut self, now: NetTime, elapsed_us: i64) {
        if self.starved > 0 && now.since(self.starved_at) >= self.margins.starve_decay_us {
            self.starved -= 1;
            self.starved_at = now;
        }
        if let Some(at) = self.margin_at
            && self.margin > 0
            && now.since(at) >= MARGIN_DECAY_US
        {
            self.margin -= 1;
            self.margin_at = Some(now);
        }
        let tick = self.tick_us();
        let move_one = |offset: &mut i64, target: Option<i64>, ppm: &mut i64| {
            *ppm = 0;
            let Some(target) = target else {
                return false;
            };
            match steer(target - *offset, tick) {
                Steer::Jump => {
                    *offset = target;
                    true
                }
                Steer::Rate(rate) => {
                    *ppm = rate;
                    *offset += elapsed_us * rate / 1_000_000;
                    false
                }
            }
        };
        let predicted_target = self.predicted_target();
        let remote_target = self.remote_target();
        let jumped = move_one(
            &mut self.predicted,
            predicted_target,
            &mut self.predicted_ppm,
        ) as u64
            + move_one(&mut self.remote, remote_target, &mut self.remote_ppm) as u64;
        self.jumps += jumped;
    }

    /// Predicted time at local time `now` (µs on the tick timeline).
    pub fn predicted_at(&self, now: NetTime) -> i64 {
        now.0 as i64 + self.predicted
    }

    /// Remote time at local time `now`.
    pub fn remote_at(&self, now: NetTime) -> i64 {
        now.0 as i64 + self.remote
    }

    /// The predicted clock's error against its target, µs.
    pub fn predicted_error(&self) -> Option<i64> {
        self.predicted_target()
            .map(|target| target - self.predicted)
    }

    /// The remote clock's error against its target, µs.
    pub fn remote_error(&self) -> Option<i64> {
        self.remote_target().map(|target| target - self.remote)
    }
}
