//! The probes of a blow that strikes a blade (`kjk sim probes --only
//! duelling`). Seat 0 swings at seat 1, who has a lit lightsaber. When seat 1
//! neither holds block nor swings, the swing goes on past the blade and hurts
//! seat 1 once, seat 1 loses `block::UNBLOCKED` Stamina once, nobody records
//! a parry, and no saber lock starts. An unblockable blow does the same harm
//! as before and records no parry. When seat 1 holds block, the blow is
//! stopped and parried, at the body or at the blade. When seat 1 swings too,
//! the two swings meet and both bounce. The decision itself is also read
//! directly: a plain swing, a fake and an unblockable blow against a player
//! or an NPC who does not guard, beside the same blows against a guard.

use super::super::duels::play;
use super::stamina::side;
use crate::Seat;
use crate::combat::Outcome;
use crate::combat::saber::duel;
use crate::combat::saber::duel::exchange::{Exchange, Guard, Side, Took, resolve};
use crate::combat::saber::duel::tap::{self, Tapped};
use crate::probe::{Checks, check};
use crate::snapshot::{SimId, Snapshot};
use rules::duel::{block, meters};

pub(super) fn probes() -> Checks {
    let mut all: Checks = STRUCK.iter().map(|row| (row.name, struck(row))).collect();
    all.push(("idle_blade_starts_no_lock", idle_blade_starts_no_lock()));
    all.push(("idle_guard_decisions", idle_guard_decisions()));
    all
}

/// What the first blow of seat 0 at seat 1 did.
#[derive(Debug, PartialEq)]
struct Struck {
    /// Seat 1's guard.
    guard: Guard,
    /// Whether the blades met.
    met: bool,
    /// Whether the blow was stopped.
    stopped: bool,
    /// Whether seat 0's move on the tick before the blow is unblockable.
    unblockable: bool,
    /// What seat 0 and seat 1 took from the exchange.
    took: [Took; 2],
    /// Whether seat 0 still plays the same move on the tick of the blow.
    swings_on: bool,
    /// How many times that move hurt seat 1 before it ended.
    hits: usize,
    /// Whether either seat recorded a parry on the tick of the blow.
    parried: bool,
    /// How many blows of the same move met seat 1.
    blows: usize,
    /// The Stamina seat 1 lost to the blow, when the row asks about it.
    lost: Option<i32>,
}

/// A row: the probe's name, how far in front of seat 0 seat 1 stands, the
/// keys seat 0 and seat 1 press during the fight, and what the first blow
/// is asked to do.
struct Row {
    name: &'static str,
    x: u32,
    swing: &'static str,
    keys: &'static str,
    want: Struck,
}

/// Seat 0 holds attack and steps to its right. Seat 1 looks 30 degrees to
/// the left of seat 0. With seat 1 50 units away, seat 0's swing meets seat
/// 1's blade before its body.
const SWING_PAST: &str = "b0=attack r0=1 t1=30";

/// What a plain swing that goes on past an idle blade does.
const PASSED: Struck = Struck {
    guard: Guard::None,
    met: true,
    stopped: false,
    unblockable: false,
    took: [Took::Nothing, Took::Nothing],
    swings_on: true,
    hits: 1,
    parried: false,
    blows: 1,
    lost: None,
};

/// What a swing stopped by a held block does.
const STOPPED: Struck = Struck {
    guard: Guard::Held,
    met: false,
    stopped: true,
    unblockable: false,
    took: [Took::Bounce(duel::Pace::Parried), Took::Parry],
    swings_on: false,
    hits: 0,
    parried: true,
    blows: 1,
    lost: None,
};

const STRUCK: [Row; 5] = [
    Row {
        name: "unguarded_blade_lets_a_swing_through",
        x: 50,
        swing: SWING_PAST,
        keys: "",
        want: Struck {
            lost: Some(block::UNBLOCKED),
            ..PASSED
        },
    },
    // A kata is unblockable. It knocks the idle seat 1 aside and costs it
    // `block::UNBLOCKABLE` Stamina, as before, but nobody parried it.
    Row {
        name: "unblockable_blow_at_an_idle_blade",
        x: 40,
        swing: "b0=attack+alt",
        keys: "t1=-30",
        want: Struck {
            unblockable: true,
            took: [Took::Nothing, Took::KnockedAside],
            lost: Some(block::UNBLOCKABLE),
            ..PASSED
        },
    },
    // A held block meets the blow at seat 1's body, before the blades meet.
    Row {
        name: "held_block_stops_a_swing",
        x: 50,
        swing: SWING_PAST,
        keys: "h1=block",
        want: STOPPED,
    },
    // Seat 0 runs in from 70 units, and seat 1 holds block looking 20
    // degrees to the right of seat 0, so the blades meet first.
    Row {
        name: "held_block_at_the_blade_stops_a_swing",
        x: 70,
        swing: "b0=attack f0=1",
        keys: "t1=-20 h1=block",
        want: Struck {
            met: true,
            ..STOPPED
        },
    },
    // Both seats swing straight at each other from 45 units, so the blades
    // meet while both swing.
    Row {
        name: "two_swings_meet_and_bounce",
        x: 45,
        swing: "b0=attack",
        keys: "b1=attack",
        want: Struck {
            guard: Guard::None,
            met: true,
            took: [Took::AttackBounce, Took::AttackBounce],
            ..STOPPED
        },
    },
];

/// A duel under the Advanced rules with seat 1 `x` units in front of seat
/// 0, under the seed `seed`. Both seats light their sabers. Then seat 0
/// presses `swing` and seat 1 presses `keys` for 300 ticks.
fn fight(seed: u64, x: u32, swing: &str, keys: &str) -> String {
    format!(
        "scenario probe\nrules advanced\nseed {seed}\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 1 medium x={x} yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         begin\ntick 1 b0=attack b1=attack\ntick 60\ntick 300 {swing} {keys}\n"
    )
}

/// A blow of seat 0 at seat 1: its tick, seat 1's guard, seat 1's Stamina
/// as the decision read it, what each side took, whether it was stopped
/// and whether the blades met.
type Blow = (u64, Guard, i32, [Took; 2], bool, bool);

/// `text` played: every tick, the id of seat 0, and every blow of seat 0 at
/// seat 1.
fn blows_of(text: &str) -> Result<(Vec<Snapshot>, SimId, Vec<Blow>), String> {
    tap::listen();
    let ticks = play(text);
    let tapped = tap::take();
    tap::stop();
    let ticks = ticks?;
    let seat = |n: u8| {
        let last = ticks.last().and_then(|tick| tick.own(Seat(n)));
        last.map(|entity| entity.id)
            .ok_or_else(|| format!("no seat {n}"))
    };
    let (a, d) = (seat(0)?, seat(1)?);
    let blows = tapped
        .into_iter()
        .filter_map(|tapped| match tapped {
            Tapped::Exchange {
                tick,
                a: by,
                d: at,
                guard,
                points,
                took,
                stopped,
                met,
                ..
            } if by == a && at == d => Some((tick, guard, points[1], took, stopped, met)),
            _ => None,
        })
        .collect();
    Ok((ticks, a, blows))
}

/// The fighter of seat `n` on every tick of `ticks`.
fn fighters_of(ticks: &[Snapshot], n: u8) -> impl Iterator<Item = crate::combat::Fighter> + '_ {
    (ticks.iter()).filter_map(move |t| t.own(Seat(n))?.fighter)
}

/// The first blow of seat 0 at seat 1 in the fight of `row`, read against
/// what the row asks.
fn struck(row: &Row) -> Result<String, String> {
    let want = &row.want;
    let (ticks, a, blows) = blows_of(&fight(7, row.x, row.swing, row.keys))?;
    let &(tick, guard, points, took, stopped, met) =
        blows.first().ok_or("seat 0 never met seat 1")?;
    let fighter = |n: u8, at: u64| {
        let snapshot = ticks.iter().find(|t| t.tick == at)?;
        snapshot.own(Seat(n))?.fighter
    };
    // The swing lasts, from the tick of the blow, while seat 0 plays the
    // move it swung on the tick before the blow.
    let swung = fighter(0, tick - 1).map(|f| f.saber.current_move);
    let swing: Vec<Snapshot> = ticks
        .iter()
        .filter(|t| t.tick >= tick)
        .take_while(|t| {
            let now = t.own(Seat(0)).and_then(|entity| entity.fighter);
            now.is_some_and(|f| swung == Some(f.saber.current_move))
        })
        .cloned()
        .collect();
    let end = swing.last().map_or(tick, |t| t.tick);
    // Each hit is one event, kept on many ticks: counted once by its number.
    let mut hits = std::collections::BTreeSet::new();
    for f in fighters_of(&swing, 1) {
        for event in f.events.iter().flatten() {
            let hit = matches!(
                event.result,
                Outcome::Hit | Outcome::Critical | Outcome::Automatic
            );
            if hit && event.by == Some(a) && event.tick >= tick {
                hits.insert(event.sequence);
            }
        }
    }
    let parried = [0, 1].into_iter().any(|n| {
        fighter(n, tick).is_some_and(|f| {
            (f.events.iter().flatten())
                .any(|event| event.tick == tick && event.result == Outcome::Parried)
        })
    });
    let after = fighter(1, tick).map(|f| i32::from(f.saber.duel.points));
    let got = Struck {
        guard,
        met,
        stopped,
        unblockable: swung.is_some_and(duel::unblockable),
        took,
        swings_on: !swing.is_empty(),
        hits: hits.len(),
        parried,
        blows: blows.iter().filter(|blow| blow.0 <= end).count(),
        lost: want.lost.and(after.map(|after| points - after)),
    };
    check(
        got == *want,
        format!(
            "the first blow, on tick {tick}, with seat 0's swing ending on tick {end}: {got:?}; expected {want:?}"
        ),
    )
}

/// The seeds the lock probe plays.
const LOCK_SEEDS: u64 = 40;

/// Seat 1 holds the walk key, so it is not a player at rest (a player at
/// rest never locks blades), and it neither blocks nor swings. Over
/// `LOCK_SEEDS` seeds, the swings of seat 0 that meet its blade are not
/// stopped, start no saber lock, and record no parry.
fn idle_blade_starts_no_lock() -> Result<String, String> {
    let (mut met, mut stopped, mut locked, mut parries) = (0, 0, 0, 0);
    for seed in 1..=LOCK_SEEDS {
        let (ticks, _, blows) = blows_of(&fight(seed, 50, SWING_PAST, "b1=walk"))?;
        met += blows.iter().filter(|blow| blow.5).count();
        stopped += blows.iter().filter(|blow| blow.4).count();
        let both = || fighters_of(&ticks, 0).chain(fighters_of(&ticks, 1));
        locked += usize::from(both().any(|f| f.saber.lock.is_some()));
        let mut seen = std::collections::BTreeSet::new();
        for (n, fighters) in [fighters_of(&ticks, 0), fighters_of(&ticks, 1)]
            .into_iter()
            .enumerate()
        {
            for f in fighters {
                for event in f.events.iter().flatten() {
                    if event.result == Outcome::Parried {
                        seen.insert((n, event.sequence));
                    }
                }
            }
        }
        parries += seen.len();
    }
    check(
        met > 0 && stopped == 0 && locked == 0 && parries == 0,
        format!(
            "over {LOCK_SEEDS} seeds: blows that met the idle blade {met}, stopped {stopped}; fights with a saber lock {locked}; parries recorded {parries}"
        ),
    )
}

/// What a decision is read for: whether no parry is recorded, whether a
/// lock is forced, whether the blow is stopped, what each side took and
/// each side's Stamina moved.
type Decided = (bool, bool, bool, [Took; 2], [i32; 2]);

fn decided(x: Exchange) -> Decided {
    (x.passed, x.forced_lock, x.stopped, x.took, x.points)
}

/// An attacking side with Stamina to spare, changed by `more`.
pub(super) fn attack(more: fn(&mut Side)) -> Side {
    let mut side = Side {
        attacking: true,
        ..side(100, Guard::None)
    };
    more(&mut side);
    side
}

/// The decision for a blow against a fighter who does not guard, beside the
/// same blow against a guard, which must not change.
fn idle_guard_decisions() -> Result<String, String> {
    let plain = attack(|_| {});
    let fake = attack(|s| s.faking = true);
    let unblockable = attack(|s| s.unblockable = true);
    let lunge = attack(|s| s.lunge = true);
    let idle = side(100, Guard::None);
    let npc = Side { npc: true, ..idle };
    let swinging = Side {
        attacking: true,
        ..idle
    };
    let wound_up = Side {
        unblockable: true,
        ..idle
    };
    let held = side(100, Guard::Held);
    let tired = side(meters::TIRED, Guard::Held);
    let (unblocked, knocked) = (-block::UNBLOCKED, -block::UNBLOCKABLE);
    let nothing = [Took::Nothing; 2];
    let aside = [Took::Nothing, Took::KnockedAside];
    let parried = [Took::Bounce(duel::Pace::Parried), Took::Parry];
    let passed = |points| (true, false, false, nothing, [0, points]);
    let rows: [(&str, Side, Side, Decided); 11] = [
        ("plain at a player", plain, idle, passed(unblocked)),
        (
            "plain at a player in an unblockable move",
            plain,
            wound_up,
            (
                true,
                false,
                false,
                [Took::HeavyStagger, Took::Nothing],
                [0, unblocked],
            ),
        ),
        ("plain at an NPC", plain, npc, passed(0)),
        ("fake at a player", fake, idle, passed(unblocked)),
        ("fake at an NPC", fake, npc, passed(0)),
        (
            "unblockable at a player",
            unblockable,
            idle,
            (true, false, false, aside, [0, knocked]),
        ),
        (
            "unblockable at a swinging player",
            unblockable,
            swinging,
            (false, false, false, aside, [0, knocked]),
        ),
        (
            "plain at a held block",
            plain,
            held,
            (
                false,
                false,
                true,
                parried,
                [-block::BLOCKED, -block::HELD[0]],
            ),
        ),
        (
            "fake at a held block",
            fake,
            held,
            (false, true, false, nothing, [0, 0]),
        ),
        (
            "unblockable at a held block",
            unblockable,
            held,
            (false, false, false, aside, [0, knocked]),
        ),
        (
            "lunge past a tired held block",
            lunge,
            tired,
            (
                false,
                false,
                false,
                [Took::AttackBounce, Took::Stagger],
                [-block::BLOCKED, unblocked],
            ),
        ),
    ];
    let mut right = true;
    let mut said = Vec::new();
    for (name, a, d, want) in rows {
        let got = decided(resolve(&a, &d, true));
        right &= got == want;
        let wrong = if got == want {
            String::new()
        } else {
            format!(", expected {want:?}")
        };
        said.push(format!("{name}: {got:?}{wrong}"));
    }
    check(right, said.join("; "))
}
