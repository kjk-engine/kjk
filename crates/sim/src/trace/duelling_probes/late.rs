//! The probes of the late perfect block and of the perfect block window
//! (`kjk sim probes --only duelling`).
//!
//! Seat 0 swings at seat 1 under the Advanced rules. A first fight finds the
//! first blow of seat 0 that seat 1 stops. The same fight is then played
//! again, with seat 1 pressing attack a set time after that blow. A press
//! within `perfect::LATE_GRACE_MS` after a blow that a held block stopped
//! makes the block perfect. A later press does not. With targeted blocks on,
//! a press aimed at the wrong side does not. A lunge is a swing from below,
//! which has no side, so a press aimed anywhere makes its block perfect. A
//! press after two swings met and bounced does not make a block perfect,
//! because seat 1 did not block the blow. A press after a lunge on a tired
//! seat 1 does not either, because the lunge was not stopped and staggers
//! seat 1. The grace itself is also read directly, at its edges, and so is
//! the decision of which blocks a late press may make perfect.
//!
//! The perfect block window stays open after seat 1 lets go of attack, and
//! closes when seat 1 lets go of block. It also closes with the first
//! perfect block made in it, so that one press of attack makes at most one
//! perfect block. This holds in a level with stance sets too, where the
//! perfect parry leaves the blocker standing ready.

use super::super::duels::play;
use super::stamina::side;
use super::unguarded::attack;
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::saber::duel::exchange::{self, Guard, Side};
use crate::combat::saber::duel::tap::{self, Tapped};
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;
use rules::duel::{meters, perfect};

pub(super) fn probes() -> Checks {
    let mut all: Checks = LATE.iter().map(|row| (row.name, late(row))).collect();
    all.push(("late_grace_edges", late_grace_edges()));
    all.push(("late_press_needs_a_stopped_blow", late_upgradeable()));
    all.extend(
        WINDOW
            .iter()
            .map(|&(name, keys, open)| (name, window(keys, open))),
    );
    all.extend(
        ONE_PRESS
            .iter()
            .map(|&(name, sets)| (name, one_press(sets))),
    );
    all
}

/// A fight in which seat 1 presses attack after a blow of seat 0, and
/// whether the press must make the blow's block perfect.
struct Late {
    name: &'static str,
    /// Whether the session plays with targeted blocks.
    targeted: bool,
    /// Seat 0's fighting style.
    kind: &'static str,
    /// The lines played before the swing, and how many steps they take.
    setup: &'static str,
    setup_steps: u64,
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
    /// Whether seat 1's block stops the blow.
    stopped: bool,
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
const INSIDE: u64 = 56;
const AFTER: u64 = 64;
/// Seat 0's keys for a lunge, once it has crouched (`CROUCH`).
const LUNGE: &str = "b0=attack f0=1 u0=-1";
/// Seat 0 takes the fast style and waits until both blades are drawn. It
/// then crouches for 20 steps while seat 1 holds down block.
const CROUCH: &str = "saber 0 style=fast\ntick 90\ntick 20 u0=-1 h1=block s1=0\n";
/// The same, with seat 1 tired.
const CROUCH_TIRED: &str =
    "saber 0 style=fast\ntick 90\nduel 1 points=0\ntick 20 u0=-1 h1=block s1=0\n";
/// The steps that `CROUCH` and `CROUCH_TIRED` take.
const CROUCH_STEPS: u64 = 110;

/// A press inside the grace after an ordinary block of the swing from the
/// left, with targeted blocks off.
const INSIDE_THE_GRACE: Late = Late {
    name: "late_press_inside_the_grace",
    targeted: false,
    kind: "medium",
    setup: "",
    setup_steps: 0,
    swing: FROM_LEFT,
    before: HOLD,
    after: HOLD,
    press_ms: INSIDE,
    held: true,
    stopped: true,
    made_perfect: true,
};

/// A press inside the grace after a lunge that seat 1 blocks, aimed to the
/// right.
const AFTER_A_LUNGE: Late = Late {
    name: "late_press_after_a_lunge_aimed_anywhere",
    targeted: true,
    kind: "fast",
    setup: CROUCH,
    setup_steps: CROUCH_STEPS,
    swing: LUNGE,
    after: HOLD_RIGHT,
    press_ms: INSIDE,
    ..INSIDE_THE_GRACE
};

const LATE: [Late; 7] = [
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
    AFTER_A_LUNGE,
    // The lunge passes the tired seat 1's block and staggers it, so seat 1
    // cannot hold block at the press either.
    Late {
        name: "late_press_after_a_lunge_on_a_tired_player",
        targeted: false,
        setup: CROUCH_TIRED,
        after: HOLD,
        held: false,
        stopped: false,
        made_perfect: false,
        ..AFTER_A_LUNGE
    },
];

/// The fight of `row` under the Advanced rules, 45 units apart. After
/// `row.setup`, seat 0 presses `row.swing` and seat 1 `row.before` for
/// `before` ticks. With
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
         fighter 0 {} x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         sheet 1 level=20\nbegin\ntick 1 b0=attack b1=attack\ntick 60\n{}\
         tick {before} {} {}\n{pressed}",
        row.kind, row.setup, row.swing, row.before
    )
}

/// The number of steps the fight takes before seat 0 starts to swing.
const SETUP_STEPS: u64 = 61;

/// Seat `n`'s fighter in `snapshot`.
fn seat(snapshot: &Snapshot, n: u8) -> Option<Fighter> {
    snapshot.own(Seat(n)).and_then(|entity| entity.fighter)
}

/// `text` played, and the tick of the first blow of seat 0 that seat 1
/// blocked, not perfectly. The block stopped the blow when `stopped` is set,
/// and did not stop it otherwise.
fn first_blocked(text: &str, stopped: bool) -> Result<(Vec<Snapshot>, u64), String> {
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
            stopped: was,
            perfect: false,
            ..
        } if Some(by) == a && Some(at) == d && was == stopped => Some(tick),
        _ => None,
    });
    let none = format!("seat 1 never blocked a blow of seat 0 with stopped {stopped}");
    Ok((ticks, blow.ok_or(none)?))
}

/// The sim time of the snapshot at `tick` among `ticks`.
fn time_at(ticks: &[Snapshot], tick: u64) -> Result<u64, String> {
    let at = ticks.iter().find(|t| t.tick == tick);
    at.map(|t| t.time_ms)
        .ok_or_else(|| format!("no snapshot at tick {tick}"))
}

fn late(row: &Late) -> Result<String, String> {
    let (first, blow) = first_blocked(&late_fight(row, 300, false), row.stopped)?;
    let step_ms = match first.as_slice() {
        [a, b, ..] => b.time_ms - a.time_ms,
        _ => return Err("the fight played fewer than two ticks".into()),
    };
    let blow_index = first.iter().position(|t| t.tick == blow).unwrap_or(0) as u64;
    let setup = SETUP_STEPS + row.setup_steps;
    let before = (blow_index + row.press_ms / step_ms).saturating_sub(setup);
    let (ticks, again) = first_blocked(&late_fight(row, before, true), row.stopped)?;
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

/// Which ordinary blocks a late press may make perfect
/// (`exchange::late_upgradeable`). A plain swing stopped by a held block may
/// be made perfect, whether or not the blocker is tired. A lunge that passes
/// a tired blocker's held block was not stopped, and two swings that meet
/// would not be blocked perfectly by a perfect guard either. An unblockable
/// blow was not stopped by the held block.
fn late_upgradeable() -> Result<String, String> {
    let plain = attack(|_| {});
    let held = side(100, Guard::Held);
    let tired = side(meters::TIRED, Guard::Held);
    let swinging = Side {
        attacking: true,
        ..held
    };
    let rows = [
        ("plain at a held block", plain, held, false, true),
        ("plain at a tired held block", plain, tired, false, true),
        (
            "lunge past a tired held block",
            attack(|s| s.lunge = true),
            tired,
            false,
            false,
        ),
        (
            "lunge at a held block",
            attack(|s| s.lunge = true),
            held,
            false,
            true,
        ),
        ("two swings meet", plain, swinging, true, false),
        (
            "unblockable at a held block",
            attack(|s| s.unblockable = true),
            held,
            false,
            false,
        ),
    ];
    let wrong: Vec<String> = (rows.iter())
        .filter_map(|&(name, a, d, met, want)| {
            let x = exchange::resolve(&a, &d, met);
            let got = exchange::late_upgradeable(&a, &d, met, &x);
            (got != want).then(|| format!("{name}: {got}, expected {want}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        match wrong.is_empty() {
            true => format!("all {} blocks as expected", rows.len()),
            false => wrong.join("; "),
        },
    )
}

/// The window probes: the keys seat 1 presses for 10 ticks after its press
/// of attack with block held, and whether its window must then still be
/// open. In the last row a party conversation starts after those 10 ticks,
/// and seat 1 goes on holding block for 2 more ticks in it.
const WINDOW: [(&str, &str, bool); 3] = [
    ("window_survives_attack_release", "h1=block", true),
    ("window_ends_with_block_release", "", false),
    (
        "window_ends_with_a_scene",
        "h1=block\ntalk party\ntick 2 h1=block",
        false,
    ),
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

/// The one press probes, and whether the level has stance sets.
const ONE_PRESS: [(&str, bool); 2] = [
    ("one_press_one_perfect_block", false),
    ("one_press_one_perfect_block_with_stance_sets", true),
];

/// The steps that light the three blades and wait until all stand ready.
const LIGHT_STEPS: u64 = 201;
/// How many steps before the first blow seat 1 clicks attack.
const CLICK_LEAD: u64 = 10;

/// Seats 0 and 2 stand side by side and swing at seat 1 together, from
/// tick `LIGHT_STEPS + 1` on, while seat 1 holds down block. With `click`
/// set, seat 1 clicks attack once, for one step, that many steps after the
/// swings start.
fn two_on_one(sets: bool, click: Option<u64>) -> String {
    let stances = if sets { "stances synthetic\n" } else { "" };
    let look = "h1=block s0=1 s1=0 s2=1 b0=attack r0=-1 b2=attack r2=-1";
    let swings = match click {
        Some(after) => format!("tick {after} {look}\ntick 1 {look} b1=attack\ntick 60 {look}\n"),
        None => format!("tick 300 {look}\n"),
    };
    format!(
        "scenario probe\nrules advanced\nseed 7\n{stances}\
         fighter 0 medium x=0 y=-15 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 2 medium x=0 y=15 defence=2 tiers=2 fp=100 hp=40000\n\
         sheet 1 level=20\nbegin\ntick 1 b0=attack b1=attack b2=attack\n\
         tick {}\n{swings}",
        LIGHT_STEPS - 1
    )
}

/// The first blow of seats 0 and 2 on seat 1 in `text`, each as its tick,
/// whether it was stopped and whether it was blocked perfectly. None: the
/// blow met no guard of seat 1.
type FirstBlows = [Option<(u64, bool, bool)>; 2];

/// `text` played, and the first blows on seat 1 (`FirstBlows`).
fn first_blows(text: &str) -> Result<(Vec<Snapshot>, FirstBlows), String> {
    tap::listen();
    let ticks = play(text);
    let tapped = tap::take();
    tap::stop();
    let ticks = ticks?;
    let last = ticks.last().ok_or("the fight played no ticks")?;
    let id = |n: u8| last.own(Seat(n)).map(|e| e.id);
    let first = |n: u8| {
        tapped.iter().find_map(|tapped| match *tapped {
            Tapped::Exchange {
                tick,
                a,
                d,
                stopped,
                perfect,
                ..
            } if Some(a) == id(n) && Some(d) == id(1) => Some((tick, stopped, perfect)),
            _ => None,
        })
    };
    let blows = [first(0), first(2)];
    Ok((ticks, blows))
}

/// A first fight finds the first blows of seats 0 and 2 on seat 1. In the
/// same fight played again, seat 1 clicks attack once, `CLICK_LEAD` steps
/// before the earlier blow. One of the two blows must be blocked perfectly
/// inside the window that the click opened. With stance sets, seat 1 stands
/// ready after the perfect parry, and the other blow must be stopped by an
/// ordinary block inside the same window. Without them, seat 1 plays the
/// knockaway, so the other blow meets no guard and is not blocked at all.
fn one_press(sets: bool) -> Result<String, String> {
    let (_, blows) = first_blows(&two_on_one(sets, None))?;
    let first = (blows.iter().flatten().map(|blow| blow.0).min())
        .ok_or_else(|| format!("seat 1 met no blow of seats 0 and 2: {blows:?}"))?;
    let after = first.saturating_sub(LIGHT_STEPS + 1 + CLICK_LEAD);
    let (ticks, blows) = first_blows(&two_on_one(sets, Some(after)))?;
    let duels = || (ticks.iter().filter_map(|t| seat(t, 1))).map(|f| f.saber.duel);
    let opened = duels().map(|duel| duel.aimed_at).max().unwrap_or(0);
    let until = duels().map(|duel| duel.perfect_until).max().unwrap_or(0);
    let inside = |tick: u64| time_at(&ticks, tick).is_ok_and(|at| at > opened && at < until);
    let met: Vec<_> = blows.iter().flatten().collect();
    let perfect = met.iter().filter(|blow| blow.2).count();
    let ordinary = met.iter().filter(|blow| blow.1 && !blow.2).count();
    check(
        met.iter().all(|blow| inside(blow.0))
            && perfect == 1
            && ordinary == usize::from(sets)
            && met.len() == 1 + usize::from(sets),
        format!(
            "the click at {opened} ms opened the window until {until} ms; the first blows \
             (tick, stopped, perfect) of seats 0 and 2 {blows:?}: {perfect} perfect and \
             {ordinary} ordinary, expected one perfect and {} ordinary inside the window",
            usize::from(sets)
        ),
    )
}
