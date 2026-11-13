//! What the blades decided when a blow met a blade, told to a measuring
//! verb as it happens (`kjk combat balance`): who swung, who guarded and
//! how, what each side took. A probes' tap: kept per thread, and only while
//! someone listens (`listen`); it reads the step and changes nothing of it.

use std::cell::RefCell;

use super::exchange::{Guard, Took};
use crate::snapshot::SimId;

/// One decision, at the tick it was made; `a` swung at `d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tapped {
    /// A duel mode's exchange (`exchange::resolve`): `d`'s guard, what each
    /// side took (attacker first), a perfect block, whether the blow was
    /// stopped, and whether the blades met (else a body blocked it).
    Exchange {
        tick: u64,
        a: SimId,
        d: SimId,
        guard: Guard,
        took: [Took; 2],
        perfect: bool,
        stopped: bool,
        met: bool,
    },
    /// JA's rules: `d` turned the blow by state (its guard arc).
    Block { tick: u64, a: SimId, d: SimId },
    /// JA's rules: the blades met in a clash.
    Clash { tick: u64, a: SimId, d: SimId },
}

thread_local! {
    static TAP: RefCell<Option<Vec<Tapped>>> = const { RefCell::new(None) };
}

/// Starts keeping this thread's decisions (any kept before are dropped).
pub fn listen() {
    TAP.with(|tap| *tap.borrow_mut() = Some(Vec::new()));
}

/// Stops keeping them.
pub fn stop() {
    TAP.with(|tap| *tap.borrow_mut() = None);
}

/// The decisions kept since the last call, oldest first.
pub fn take() -> Vec<Tapped> {
    TAP.with(|tap| {
        tap.borrow_mut()
            .as_mut()
            .map_or_else(Vec::new, std::mem::take)
    })
}

/// Keeps `tapped` if this thread listens.
#[cfg(feature = "probes")]
pub(crate) fn tell(tapped: Tapped) {
    TAP.with(|tap| {
        if let Some(kept) = tap.borrow_mut().as_mut() {
            kept.push(tapped);
        }
    });
}
