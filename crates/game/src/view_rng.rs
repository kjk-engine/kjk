//! The view's repeatable numbers: drawn from what every screen shares (a
//! character, a tick, a moment), so every screen and every replay draws
//! the same, and never from the sim's streams, which a view must not move.

use asset_ja::fx::Rng;

/// 2^32 over the golden ratio: spreads a small number over a word.
const GOLDEN_32: u32 = 0x9e37_79b9;

/// `sim::rng`'s SplitMix64 finaliser.
pub(crate) use sim::rng::{GOLDEN_64, mix64};

/// JA's effect generator seeded from `a`, spread by the golden ratio, and
/// `b`.
pub(crate) fn ja_seed(a: u32, b: u32) -> Rng {
    Rng((a.wrapping_mul(GOLDEN_32) ^ b) as i32)
}

/// A word's bits spread over every bit out (the "lowbias32" mix).
fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// `mix32` of `a` and `salt` spread by the golden ratio.
pub(crate) fn mix32_salted(a: u32, salt: u32) -> u32 {
    mix32(a ^ salt.wrapping_mul(GOLDEN_32))
}
