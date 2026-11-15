//! The probes of the late perfect block and of the perfect block window
//! (`kjk sim probes --only duelling`).
//!
//! Seat 0 swings at seat 1 under the Advanced rules. A first fight finds the
//! first blow of seat 0 that seat 1 stops. The same fight is then played
//! again, with seat 1 pressing attack a set time after that blow. A press
//! within `perfect::LATE_GRACE_MS` after a blow that a held block stopped
//! makes the block perfect. A later press does not. With targeted blocks on,
//! a press aimed at the wrong side does not. A press after two swings met
//! and bounced does not either, because seat 1 did not block the blow. The
//! grace itself is also read directly, at its edges.
//!
//! The perfect block window stays open after seat 1 lets go of attack, and
//! closes when seat 1 lets go of block.

use super::super::duels::play;
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::saber::duel::exchange::{self, Guard};
use crate::combat::saber::duel::tap::{self, Tapped};
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;
use rules::duel::perfect;

pub(super) fn probes() -> Checks {
    let mut all: Checks = LATE.iter().map(|row| (row.name, late(row))).collect();
    all.push(("late_grace_edges", late_grace_edges()));
    all.extend(
        WINDOW
            .iter()
            .map(|&(name, keys, open)| (name, window(keys, open))),
    );
    all
}

/// A fight in which seat 1 presses attack after a blow of seat 0, and
/// whether the press must make the blow's block perfect.
struct Late {
    name: &'static str,
    /// Whether the session plays with targeted blocks.
    targeted: bool,
    /// Seat 0's keys until seat 1's press. Seat 0 lets go of everything
    /// from the press on, so that no second blow meets seat 1.
    swing: &'static str,
    /// Seat 1's keys before the press.
    before: &'static str,
    /// Seat 1's keys from the press on. Attack is added to them for the
    /// press itself.
    after: &'static str,
    /// How long after the blow seat 1 presses attack (ms).
    press_ms: u64,
    /// Whether seat 1 can hold block at the press, so that the press opens
    /// a perfect block window.
    held: bool,
    made_perfect: bool,
}

/// Seat 1 holds down block and looks at seat 0.
const HOLD: &str = "h1=block s1=0";
/// Seat 1 holds down block, looks at seat 0 and turns its camera 15
/// degrees to the left, or to the right.
const HOLD_LEFT: &str = "h1=block s1=0 t1=15";
const HOLD_RIGHT: &str = "h1=block s1=0 t1=-15";
/// Seat 0's swing from the right to its left, which comes from seat 1's
/// left.
const FROM_LEFT: &str = "b0=attack r0=-1";
/// A press inside the late grace, and the first press after it, at the
/// sim's step of 8 ms.
const INSIDE: u64 = 144;
const AFTER: u64 = 152;

/// A press inside the grace after an ordinary block of the swing from the
/// left, with targeted blocks off.
const INSIDE_THE_GRACE: Late = Late {
    name: "late_press_inside_the_grace",
    targeted: false,
    swing: FROM_LEFT,
    before: HOLD,
    after: HOLD,
    press_ms: INSIDE,
    held: true,
    made_perfect: true,
};

const LATE: [Late; 5] = [
    INSIDE_THE_GRACE,
    Late {
        name: "late_press_after_the_grace",
        press_ms: AFTER,
        made_perfect: false,
        ..INSIDE_THE_GRACE
    },
    Late {
        name: "late_press_aimed_with_targeted_blocks",
        targeted: true,
        after: HOLD_LEFT,
        ..INSIDE_THE_GRACE
    },
    Late {
        name: "late_press_aimed_wrong_with_targeted_blocks",
        targeted: true,
        after: HOLD_RIGHT,
        made_perfect: false,
        ..INSIDE_THE_GRACE
    },
    // Both seats swing, and the blades meet while both swing, so both
    // bounce and both record a parry. Seat 1 then holds down block and presses
    // attack. Seat 1 still bounces, so it cannot hold block, and the press
    // opens no window.
    Late {
        name: "late_press_after_two_swings_meet",
        swing: "b0=attack",
        before: "b1=attack",
        after: HOLD,
        held: false,
        made_perfect: false,
        ..INSIDE_THE_GRACE
    },
];

/// The fight of `row` under the Advanced rules, 45 units apart. Seat 0
/// presses `row.swing` and seat 1 `row.before` for `before` ticks. With
/// `press` set, seat 1 then presses attack with `row.after` for 3 ticks, and
/// presses `row.after` for 60 more.
fn late_fight(row: &Late, before: u64, press: bool) -> String {
    let flag = if row.targeted { "on" } else { "off" };
    let pressed = match press {
        true => format!(
            "tick 3 {after} b1=attack\ntick 60 {after}\n",
            after = row.after
        ),
        false => String::new(),
    };
    format!(
        "scenario probe\nrules advanced\nseed 7\ntargeted {flag}\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         sheet 1 level=20\nbegin\ntick 1 b0=attack b1=attack\ntick 60\n\
         tick {before} {} {}\n{pressed}",
        row.swing, row.before
    )
}

/// The number of steps the fight takes before seat 0 starts to swing.
const SETUP_STEPS: u64 = 61;

/// Seat `n`'s fighter in `snapshot`.
fn seat(snapshot: &Snapshot, n: u8) -> Option<Fighter> {
    snapshot.own(Seat(n)).and_then(|entity| entity.fighter)
}

/// `text` played, and the tick of the first blow of seat 0 that seat 1
/// stopped.
fn first_stopped(text: &str) -> Result<(Vec<Snapshot>, u64), String> {
    tap::listen();
    let ticks = play(text);
    let tapped = tap::take();
    tap::stop();
    let ticks = ticks?;
    let id = |n: u8| ticks.last().and_then(|t| t.own(Seat(n))).map(|e| e.id);
    let (a, d) = (id(0), id(1));
    let blow = tapped.into_iter().find_map(|tapped| match tapped {
        Tapped::Exchange {
            tick,
            a: by,
            d: at,
            stopped: true,
            perfect: false,
            ..
        } if Some(by) == a && Some(at) == d => Some(tick),
        _ => None,
    });
    Ok((ticks, blow.ok_or("seat 1 never stopped a blow of seat 0")?))
}

/// The sim time of the snapshot at `tick` among `ticks`.
fn time_at(ticks: &[Snapshot], tick: u64) -> Result<u64, String> {
    let at = ticks.iter().find(|t| t.tick == tick);
    at.map(|t| t.time_ms)
        .ok_or_else(|| format!("no snapshot at tick {tick}"))
}

fn late(row: &Late) -> Result<String, String> {
    let (first, blow) = first_stopped(&late_fight(row, 300, false))?;
    let step_ms = match first.as_slice() {
        [a, b, ..] => b.time_ms - a.time_ms,
        _ => return Err("the fight played fewer than two ticks".into()),
    };
    let blow_index = first.iter().position(|t| t.tick == blow).unwrap_or(0) as u64;
    let before = (blow_index + row.press_ms / step_ms).saturating_sub(SETUP_STEPS);
    let (ticks, again) = first_stopped(&late_fight(row, before, true))?;
    let blow_at = time_at(&ticks, again)?;
    let blocker: Vec<Fighter> = ticks.iter().filter_map(|t| seat(t, 1)).collect();
    let pressed_at = (blocker.iter())
        .map(|f| f.saber.duel.aimed_at)
        .find(|&at| at > blow_at);
    let made = (blocker.iter()).any(|f| {
        let at = f.saber.duel.perfect_at;
        at > blow_at && at <= blow_at + perfect::LATE_GRACE_MS + step_ms
    });
    let late = pressed_at.map(|at| at - blow_at);
    check(
        again == blow && late == row.held.then_some(row.press_ms) && made == row.made_perfect,
        format!(
            "the blow on tick {blow} (again on tick {again}), attack pressed with block held \
             {late:?} ms after it: made perfect {made}, expected {} after {} ms, held {}",
            row.made_perfect, row.press_ms, row.held
        ),
    )
}

/// The late grace at its edges. A press on the step of the blow is not
/// late. A press 1 ms or `perfect::LATE_GRACE_MS` after the blow is inside
/// the grace, and one 1 ms more is not.
fn late_grace_edges() -> Result<String, String> {
    let grace = perfect::LATE_GRACE_MS;
    let rows = [(0, false), (1, true), (grace, true), (grace + 1, false)];
    let got = rows.map(|(ms, _)| (ms, exchange::in_late_grace(ms)));
    check(
        got == rows,
        format!("a press so many ms after the blow, and whether it is in the grace: {got:?}"),
    )
}

/// The window probes: the keys seat 1 presses for 10 ticks after its press
/// of attack with block held, and whether its window must then still be
/// open.
const WINDOW: [(&str, &str, bool); 2] = [
    ("window_survives_attack_release", "h1=block", true),
    ("window_ends_with_block_release", "", false),
];

/// Seat 1 holds down block for 5 ticks, presses attack with it for one
/// tick, then presses `keys` for 10 ticks. Seat 0 stands still. The probe
/// reads the window seat 1 opened, and whether the window is still open
/// with a perfect guard after the 10 ticks.
fn window(keys: &str, open: bool) -> Result<String, String> {
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         begin\ntick 1 b0=attack b1=attack\ntick 60\n\
         tick 5 h1=block\ntick 1 h1=block b1=attack\ntick 10 {keys}\n"
    );
    let ticks = play(&text)?;
    let last = ticks.last().ok_or("the fight played no ticks")?;
    let now = last.time_ms;
    let blocker = seat(last, 1).ok_or("seat 1 has no fighter")?;
    let duel = blocker.saber.duel;
    let opened = (ticks.iter().filter_map(|t| seat(t, 1)))
        .map(|f| f.saber.duel.perfect_until)
        .max()
        .unwrap_or(0);
    let guard = duel.guard(now);
    let still_open = now < duel.perfect_until && guard == Guard::Perfect;
    check(
        opened > now && still_open == open,
        format!(
            "the window opened until {opened} ms; 80 ms after the press, at {now} ms, it runs \
             until {} ms with the guard {guard:?}, open {still_open}, expected {open}",
            duel.perfect_until
        ),
    )
}
