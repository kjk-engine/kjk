//! Tick timing: which whole milliseconds each tick covers, and the
//! fixed-step accumulator that turns wall time into ticks.
//!
//! The sim counts time in whole milliseconds, as JA's movement code does. A
//! rate that does not divide 1000 gets ticks of unequal length that still sum
//! exactly: at 60 Hz the ticks are 16, 17 and 17 ms, 50 ms every three.

use std::time::Duration;

/// Ticks per second: the only place the rate is set (the rulebook's
/// `engine.tick.hz`). `kjk sim rates` measures the others.
pub const TICK_HZ: u32 = rules::engine::tick::HZ;

/// Most ticks one `advance` hands out. After a longer stall (a hitch, a
/// debugger, a slow load) the rest of the time is dropped rather than
/// simulated in a burst that would stall the next frame too.
pub const MAX_CATCH_UP: u32 = rules::engine::tick::MOST_CATCH_UP;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickRate {
    hz: u32,
}

impl Default for TickRate {
    fn default() -> Self {
        TickRate { hz: TICK_HZ }
    }
}

impl TickRate {
    /// A rate from 1 to 1000 ticks per second.
    pub fn new(hz: u32) -> Option<Self> {
        (1..=1000).contains(&hz).then_some(TickRate { hz })
    }

    pub fn hz(self) -> u32 {
        self.hz
    }

    /// Sim time, in milliseconds, when `tick` ends (tick 0 is the start).
    pub fn time_ms(self, tick: u64) -> u64 {
        tick * 1000 / u64::from(self.hz)
    }

    /// How many milliseconds `tick` (counted from 1) covers.
    pub fn msec(self, tick: u64) -> u32 {
        assert!(tick > 0, "tick 0 is the start, not a step");
        (self.time_ms(tick) - self.time_ms(tick - 1)) as u32
    }
}

/// Wall time in, whole ticks out. The remainder is kept, in nanoseconds, so
/// sim time follows wall time exactly over any stretch without a stall.
#[derive(Debug, Clone)]
pub struct Accumulator {
    rate: TickRate,
    /// The last tick handed out.
    tick: u64,
    residual_ns: u64,
    /// Wall time thrown away by the catch-up cap.
    dropped_ns: u64,
}

impl Accumulator {
    /// Starts after `tick`, the tick the world is at.
    pub fn new(rate: TickRate, tick: u64) -> Self {
        Accumulator {
            rate,
            tick,
            residual_ns: 0,
            dropped_ns: 0,
        }
    }

    /// Adds `elapsed` wall time and returns the ticks now due, in order,
    /// each with its milliseconds.
    pub fn advance(&mut self, elapsed: Duration) -> Vec<(u64, u32)> {
        self.residual_ns += elapsed.as_nanos() as u64;
        let mut due = Vec::new();
        loop {
            let msec = self.rate.msec(self.tick + 1);
            let needed = u64::from(msec) * 1_000_000;
            if self.residual_ns < needed {
                break;
            }
            if due.len() as u32 == MAX_CATCH_UP {
                self.dropped_ns += self.residual_ns;
                self.residual_ns = 0;
                break;
            }
            self.residual_ns -= needed;
            self.tick += 1;
            due.push((self.tick, msec));
        }
        due
    }

    /// How far wall time is into the next tick, from 0 to 1: what the
    /// presentation blends the last two snapshots by.
    pub fn alpha(&self) -> f32 {
        let next = u64::from(self.rate.msec(self.tick + 1)) * 1_000_000;
        (self.residual_ns as f64 / next as f64) as f32
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn dropped(&self) -> Duration {
        Duration::from_nanos(self.dropped_ns)
    }
}
