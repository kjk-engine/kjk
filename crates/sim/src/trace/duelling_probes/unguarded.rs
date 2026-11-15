//! The probes of a blow that strikes a blade (`kjk sim probes --only
//! duelling`). Seat 0 swings at seat 1, who has a lit lightsaber. When seat 1
//! does not hold block and does not swing, the swing goes on past the blade
//! and hurts seat 1, who loses `block::UNBLOCKED` Stamina once, and nobody
//! parries. When seat 1 holds block, the blow is stopped and parried. When
//! seat 1 swings too, the two swings meet and both bounce.

use super::super::duels::play;
use crate::Seat;
use crate::combat::Outcome;
use crate::combat::saber::duel::exchange::Guard;
use crate::combat::saber::duel::tap::{self, Tapped};
use crate::probe::{Checks, check};
use crate::snapshot::{SimId, Snapshot};
use rules::duel::block;

pub(super) fn probes() -> Checks {
    STRUCK.iter().map(|row| (row.name, struck(row))).collect()
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
    /// Whether seat 0 is still swinging on the tick of the blow.
    swings_on: bool,
    /// Whether the swing hurt seat 1 before it ended.
    hurt: bool,
    /// Whether either seat recorded a parry on the tick of the blow.
    parried: bool,
    /// How many blows of the same swing met seat 1.
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

const STRUCK: [Row; 3] = [
    Row {
        name: "unguarded_blade_lets_a_swing_through",
        x: 50,
        swing: SWING_PAST,
        keys: "",
        want: Struck {
            guard: Guard::None,
            met: true,
            stopped: false,
            swings_on: true,
            hurt: true,
            parried: false,
            blows: 1,
            lost: Some(block::UNBLOCKED),
        },
    },
    // A held block meets the blow at seat 1's body, before the blades meet.
    Row {
        name: "held_block_stops_a_swing",
        x: 50,
        swing: SWING_PAST,
        keys: "h1=block",
        want: Struck {
            guard: Guard::Held,
            met: false,
            stopped: true,
            swings_on: false,
            hurt: false,
            parried: true,
            blows: 1,
            lost: None,
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
            stopped: true,
            swings_on: false,
            hurt: false,
            parried: true,
            blows: 1,
            lost: None,
        },
    },
];

/// A duel under the Advanced rules with seat 1 `row.x` units in front of
/// seat 0. Both seats light their sabers. Then seat 0 presses `row.swing`
/// and seat 1 presses `row.keys` for 300 ticks.
fn fight(row: &Row) -> String {
    let Row { x, swing, keys, .. } = row;
    format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 1 medium x={x} yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         begin\ntick 1 b0=attack b1=attack\ntick 60\ntick 300 {swing} {keys}\n"
    )
}

/// The first blow of seat 0 at seat 1 in the fight of `row`, read against
/// what the row asks.
fn struck(row: &Row) -> Result<String, String> {
    let want = &row.want;
    tap::listen();
    let ticks = play(&fight(row));
    let tapped = tap::take();
    tap::stop();
    let ticks = ticks?;
    let seat = |n: u8| -> Result<SimId, String> {
        let last = ticks.last().and_then(|tick| tick.own(Seat(n)));
        last.map(|entity| entity.id)
            .ok_or_else(|| format!("no seat {n}"))
    };
    let (a, d) = (seat(0)?, seat(1)?);
    let blows: Vec<_> = tapped
        .into_iter()
        .filter_map(|tapped| match tapped {
            Tapped::Exchange {
                tick,
                a: by,
                d: at,
                guard,
                points,
                stopped,
                met,
                ..
            } if by == a && at == d => Some((tick, guard, points[1], stopped, met)),
            _ => None,
        })
        .collect();
    let &(tick, guard, points, stopped, met) = blows.first().ok_or("seat 0 never met seat 1")?;
    let fighter = |n: u8, at: u64| {
        let snapshot = ticks.iter().find(|t| t.tick == at)?;
        snapshot.own(Seat(n))?.fighter
    };
    // The swing lasts, from the tick of the blow, while seat 0 plays the
    // move it swung on the tick before the blow.
    let swung = fighter(0, tick - 1).map(|f| f.saber.current_move);
    let swing: Vec<&Snapshot> = ticks
        .iter()
        .filter(|t| t.tick >= tick)
        .take_while(|t| {
            let now = t.own(Seat(0)).and_then(|entity| entity.fighter);
            now.is_some_and(|f| swung == Some(f.saber.current_move))
        })
        .collect();
    let end = swing.last().map_or(tick, |t| t.tick);
    let hurt = swing.iter().any(|t| {
        let events = t.own(Seat(1)).and_then(|entity| entity.fighter);
        events.is_some_and(|f| {
            f.events.iter().flatten().any(|event| {
                event.by == Some(a)
                    && event.tick >= tick
                    && matches!(
                        event.result,
                        Outcome::Hit | Outcome::Critical | Outcome::Automatic
                    )
            })
        })
    });
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
        swings_on: !swing.is_empty(),
        hurt,
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
