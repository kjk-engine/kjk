//! The final hash of the pinned determinism run, alone on stdout: what
//! `cargo xtask pin` writes into the workflows (`bumps/README.md`).

use sim::determinism::{PINNED_TICKS, Setup};

fn main() {
    println!(
        "{}",
        sim::determinism::run(&Setup::pinned(), PINNED_TICKS).final_hash
    );
}
