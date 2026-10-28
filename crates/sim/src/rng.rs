//! The sim's random numbers: a permuted congruential generator (PCG32,
//! the XSH-RR output), seeded through SplitMix64. Its whole state is two
//! integers, saved in every snapshot, so a restored world draws the same
//! numbers the original would have.
//!
//! There are two streams. `shared` feeds systems that run on every step,
//! prediction included; `authority` feeds what only the authority runs
//! (scripts, spawners). A predicting client never touches `authority`, so
//! its `shared` stream stays in step with the host's.

use crate::wire::{DecodeError, Reader, Writer};

const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimRng {
    state: u64,
    /// Odd; selects one of 2^63 sequences.
    stream: u64,
}

impl SimRng {
    /// A generator for `seed`, on the sequence `stream` names.
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut mix = seed ^ stream.rotate_left(32);
        let mut rng = SimRng {
            state: 0,
            stream: (splitmix64(&mut mix) << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(splitmix64(&mut mix));
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULTIPLIER).wrapping_add(self.stream);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        xorshifted.rotate_right((old >> 59) as u32)
    }

    /// Uniform in `0..bound` without modulo bias; `bound` is at least 1.
    pub fn below(&mut self, bound: u32) -> u32 {
        assert!(bound > 0, "below(0) has no values");
        // Reject the low values that would make some results more likely.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u32();
            if value >= threshold {
                return value % bound;
            }
        }
    }

    /// Uniform in `[0, 1)`, from 24 bits, so every value is exact in f32.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[low, high)`.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u64(self.state);
        out.u64(self.stream);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Self, DecodeError> {
        let at = input.at();
        let rng = SimRng {
            state: input.u64()?,
            stream: input.u64()?,
        };
        if rng.stream & 1 == 0 {
            return Err(input.invalid("rng stream (even)", at));
        }
        Ok(rng)
    }
}

/// SplitMix64's step: the golden ratio's 64 bits on, then `mix64`.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(GOLDEN_64);
    mix64(*state)
}

/// 2^64 over the golden ratio, odd: SplitMix64's step.
pub const GOLDEN_64: u64 = 0x9e37_79b9_7f4a_7c15;

/// SplitMix64's finaliser: every bit of `z` spread over every bit out.
pub fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A draw with no stream: a number in `[0, 1)` (24 bits, exact in f32) from
/// a time window's number, an entity and a salt, the same on every machine
/// and drawing nothing (minds' aim, swings and choices: what prediction
/// may repeat).
pub fn hashed(window: u64, id: u32, salt: u64) -> f32 {
    let z = window
        .wrapping_mul(GOLDEN_64)
        .wrapping_add(u64::from(id).wrapping_mul(0xd1b5_4a32_d192_ed03))
        .wrapping_add(salt);
    (mix64(z) >> 40) as f32 / (1u64 << 24) as f32
}
