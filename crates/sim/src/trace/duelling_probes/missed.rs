//! The probes of the missed perfect block (`kjk sim probes --only
//! duelling`, `perfect::MISS_COST`).
//!
//! Seat 1 holds block with a lightsaber, presses attack once and then plays
//! the keys of a row. A press that ends without a perfect block or a turned
//! bolt costs seat 1 `perfect::MISS_COST` Stamina, once. The press ends when
//! its window has run out and attack is let go, or when seat 1 lets go of
//! block. While attack is still held the press costs nothing yet. A press
//! whose block turns a pistol's bolts back costs nothing at all.

use super::super::duels::play;
use crate::Seat;
use crate::combat::Outcome;
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;
use rules::duel::{perfect, refill};

pub(super) fn probes() -> Checks {
    MISSED.iter().map(|row| (row.name, missed(row))).collect()
}

/// A press of attack with block held, the keys seat 1 plays after it, and
/// the Stamina the press must cost.
struct Missed {
    name: &'static str,
    /// Seat 0 holds a pistol and shoots at seat 1 for the first 60 steps
    /// after the press. Otherwise seat 0 holds a lightsaber and stands still.
    shoots: bool,
    /// The lines seat 1 plays after its press.
    after: &'static str,
    /// The Stamina the press costs.
    cost: i32,
    /// How much less the cost may read: a player who lets go of block
    /// refills on the step the press is paid for.
    slack: i32,
}

const MISSED: [Missed; 4] = [
    Missed {
        name: "missed_press_costs_stamina",
        shoots: false,
        after: "tick 150 h1=block",
        cost: perfect::MISS_COST,
        slack: 0,
    },
    Missed {
        name: "missed_press_costs_at_block_release",
        shoots: false,
        after: "tick 10 h1=block\ntick 150",
        cost: perfect::MISS_COST,
        slack: refill::GAIN,
    },
    Missed {
        name: "held_press_costs_nothing_yet",
        shoots: false,
        after: "tick 150 h1=block b1=attack",
        cost: 0,
        slack: 0,
    },
    Missed {
        name: "press_that_turns_bolts_costs_nothing",
        shoots: true,
        after: "tick 150 h1=block b1=attack\ntick 150 h1=block",
        cost: 0,
        slack: 0,
    },
];

/// The steps before seat 1's press: 61 to settle the fight and light the
/// blades, then 5 with block held.
const BEFORE_PRESS: usize = 66;

/// Seat `n`'s Stamina on every step of `ticks`.
fn points(ticks: &[Snapshot], n: u8) -> Vec<i32> {
    (ticks.iter())
        .filter_map(|t| t.own(Seat(n)).and_then(|e| e.fighter))
        .map(|f| i32::from(f.saber.duel.points))
        .collect()
}

/// The bolts seat 1 turned in `ticks`.
fn turned(ticks: &[Snapshot]) -> usize {
    let last = ticks
        .last()
        .and_then(|t| t.own(Seat(1)))
        .and_then(|e| e.fighter);
    (last.iter())
        .flat_map(|f| f.events.iter().flatten())
        .filter(|event| event.result == Outcome::Deflected)
        .count()
}

/// The row played. Without shots, the Stamina is read before the press and
/// at its lowest after it, since only the press spends any and a refill only
/// raises it. With shots, each turned bolt costs Stamina too, so it is read
/// once the bolts have all landed, while attack is still held, and at the
/// end, after attack is let go.
fn missed(row: &Missed) -> Result<String, String> {
    let gunner = if row.shoots { "pistol" } else { "medium" };
    let shots = if row.shoots {
        "tick 1 h1=block b1=attack b0=attack\ntick 59 h1=block b1=attack b0=attack\n"
    } else {
        "tick 1 h1=block b1=attack\n"
    };
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 {gunner} x=0 hp=40000\n\
         fighter 1 medium x=200 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         sheet 1 level=20\nbegin\ntick 1 b1=attack\ntick 60\n\
         tick 5 h1=block\n{shots}{}\n",
        row.after
    );
    let ticks = play(&text)?;
    let points = points(&ticks, 1);
    let (from, to) = if row.shoots {
        // The bolts land within the 150 steps attack is held.
        let held = BEFORE_PRESS + 150;
        (points.get(held).copied(), points.last().copied())
    } else {
        (
            points.get(BEFORE_PRESS - 1).copied(),
            points[BEFORE_PRESS..].iter().min().copied(),
        )
    };
    let (Some(from), Some(to)) = (from, to) else {
        return Err(format!("the fight played only {} steps", points.len()));
    };
    let bolts = turned(&ticks);
    check(
        (row.cost - row.slack..=row.cost).contains(&(from - to)) && (!row.shoots || bolts > 0),
        format!(
            "seat 1's Stamina went from {from} to {to}, a cost of {}, expected {} (less up to {}); \
             {bolts} bolts turned",
            from - to,
            row.cost,
            row.slack
        ),
    )
}
