//! The probes of how Stamina comes back and what a block does with none
//! (`kjk sim probes --only duelling`): the refill by how a fighter moves,
//! the same for an enemy, an enemy at no Stamina that still swings, an
//! empty block, and a companion a player's seat leads that blocks as the
//! player does.

use super::super::duels::play;
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::saber::duel::exchange::{Guard, Side, Took, resolve};
use crate::combat::saber::duel::tap::{self, Tapped};
use crate::module::ObjectKind;
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, Snapshot};
use rules::duel::{meters, refill};

/// How many ticks make a second.
const TICKS_PER_SECOND: u64 = 125;
/// The ticks of a refill's window.
const WINDOW: u64 = 2 * TICKS_PER_SECOND;
/// How far a refill's count may stray from the rule's, in points: a stance
/// takes a moment to be taken.
const SLACK: i32 = 3;
/// The Stamina a refill starts from.
const START: i32 = 10;
/// The Stamina an idle creature starts from: above what it meditates under
/// (`npc::MEDITATE`), which would add to the refill.
const IDLE_START: i32 = 40;
/// The ticks the settling of a fight takes before the probe's own begin.
const SETTLED: usize = 61;

pub(super) fn probes() -> Checks {
    vec![
        ("refill_by_stance", refill_by_stance()),
        ("enemies_refill", enemies_refill()),
        (
            "empty_fighter_keeps_swinging",
            empty_fighter_keeps_swinging(),
        ),
        ("empty_block_outcomes", empty_block_outcomes()),
        ("empty_block_in_the_world", empty_block_in_the_world()),
        ("refused_disarm_knocks_down", refused_disarm_knocks_down()),
        ("driven_companion_blocks", driven_companion_blocks()),
    ]
}

/// Seat 0's Stamina on every tick of `ticks`.
fn seat_points(ticks: &[Snapshot]) -> Vec<i32> {
    ticks
        .iter()
        .filter_map(|tick| tick.own(Seat(0)).and_then(|entity| entity.fighter))
        .map(|f| i32::from(f.saber.duel.points))
        .collect()
}

/// The first creature's fighter on every tick of `ticks`.
fn creature_fighters(ticks: &[Snapshot]) -> Vec<Fighter> {
    ticks
        .iter()
        .filter_map(|tick| {
            tick.entities
                .iter()
                .find(|e| e.kind == Kind::Object(ObjectKind::Creature, 0))
                .and_then(|e| e.fighter)
        })
        .collect()
}

/// What the rules ask a window of `WINDOW` ticks to refill at `percent` of
/// the standing rate.
fn refilled_in_window(percent: u32) -> i32 {
    let window_ms = WINDOW * 1000 / TICKS_PER_SECOND;
    let steps = window_ms * u64::from(percent) / 100 / refill::TICK_MS;
    steps as i32 * refill::GAIN
}

/// A lone seat 0 at `START` Stamina, doing `keys` for the window: the
/// Stamina it gained.
fn gained(keys: &str) -> Result<i32, String> {
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
         begin\ntick 1\ntick 60\nduel 0 points={START}\ntick {WINDOW} {keys}\n"
    );
    let points = seat_points(&play(&text)?);
    Ok(points.last().copied().unwrap_or(0) - START)
}

/// Stamina refills at the standing rate scaled by how the fighter moves
/// (`refill::BY_STANCE`: crouched, standing, walking, running), and not at
/// all while it holds block.
fn refill_by_stance() -> Result<String, String> {
    let [crouched, standing, walking, running] = refill::BY_STANCE;
    let rows = [
        ("crouched", "u0=-1", crouched),
        ("standing", "", standing),
        ("walking", "f0=64 b0=walk", walking),
        ("running", "f0=127", running),
        ("holding block", "h0=block", 0),
    ];
    let mut right = true;
    let mut said = Vec::new();
    let mut counts = Vec::new();
    for (name, keys, percent) in rows {
        let want = refilled_in_window(percent);
        let got = gained(keys)?;
        right &= (got - want).abs() <= SLACK;
        counts.push(got);
        said.push(format!("{name} {got} (the rules ask {want})"));
    }
    right &= counts[0] > counts[1] && counts[1] > counts[2] && counts[2] > counts[3];
    check(
        right,
        format!(
            "Stamina refilled from {START} in {WINDOW} ticks: {}",
            said.join(", ")
        ),
    )
}

/// A creature with `IDLE_START` Stamina standing idle refills at the standing
/// rate, as a player does.
fn enemies_refill() -> Result<String, String> {
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=100 defence=2 tiers=2 fp=100 hp=400\n\
         creature medium faction=friendly x=0 yaw=0 defence=2 tiers=2 hp=400\n\
         begin\ntick 1\ntick 60\nduel c0 points={IDLE_START}\ntick {WINDOW}\n"
    );
    let ticks = play(&text)?;
    let last = creature_fighters(&ticks)
        .last()
        .map_or(0, |f| i32::from(f.saber.duel.points));
    let (got, want) = (last - IDLE_START, refilled_in_window(refill::BY_STANCE[1]));
    check(
        (got - want).abs() <= SLACK,
        format!(
            "a creature standing idle from {IDLE_START} Stamina refilled {got} in {WINDOW} ticks, the rules ask {want}"
        ),
    )
}

/// How long a creature with no Stamina waits before its first plain swing
/// (ms after the fight began), and how many swings it begins in all, in
/// `ticks` of a fight against a seat that does not strike back.
fn swings_at_no_stamina(ticks: u64) -> Result<(Option<u64>, usize), String> {
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         creature medium x=45 yaw=180 defence=2 tiers=2 hp=40000\n\
         begin\ntick 1\ntick 60\nduel c0 points=0\ntick {ticks} c0=0\n"
    );
    let fighters = creature_fighters(&play(&text)?);
    let mut began = Vec::new();
    let mut was = false;
    for (at, f) in fighters.iter().enumerate().skip(SETTLED) {
        let swinging = f.saber.current_move.pure_attack();
        if swinging && !was {
            began.push((at - SETTLED) as u64 * 1000 / TICKS_PER_SECOND);
        }
        was = swinging;
    }
    Ok((began.first().copied(), began.len()))
}

/// An enemy with no Stamina against a fighter in melee swings within ten
/// seconds, and goes on swinging: it backs off to recover, which ends.
fn empty_fighter_keeps_swinging() -> Result<String, String> {
    let (first, swings) = swings_at_no_stamina(20 * TICKS_PER_SECOND)?;
    check(
        first.is_some_and(|ms| ms <= 10_000) && swings >= 3,
        format!(
            "an enemy with 0 Stamina began its first plain swing after {first:?} ms, and {swings} swings in 20 s"
        ),
    )
}

/// A side of a blow for the exchange's decision.
fn side(points: i32, guard: Guard) -> Side {
    Side {
        npc: false,
        points,
        chance: 500,
        critical: false,
        heavy: false,
        power: 1,
        attacking: false,
        faking: false,
        kata: false,
        unblockable: false,
        lunge: false,
        super_break: false,
        guard,
    }
}

/// A block held with no Stamina still stops a blow: a plain swing staggers
/// the blocker, a heavy swing knocks it down and a critical hit disarms it.
/// With Stamina the same blows are blocked as before. A lunge still passes
/// a tired blocker.
fn empty_block_outcomes() -> Result<String, String> {
    use Took::{Disarm, KnockedDown, Stagger};
    /// A blow: the blocker's Stamina, and whether the blow is heavy, a
    /// critical hit or a lunge.
    struct Blow {
        points: i32,
        heavy: bool,
        critical: bool,
        lunge: bool,
    }
    let blow = |points, heavy, critical, lunge| Blow {
        points,
        heavy,
        critical,
        lunge,
    };
    // (the blow, whether it is stopped, what an empty block makes of the
    // blocker)
    let rows = [
        (blow(0, false, false, false), true, Some(Stagger)),
        (blow(3, false, false, false), true, Some(Stagger)),
        (blow(0, true, false, false), true, Some(KnockedDown)),
        (blow(0, false, true, false), true, Some(Disarm)),
        (blow(0, true, true, false), true, Some(Disarm)),
        (blow(50, false, false, false), true, None),
        (blow(50, true, false, false), true, None),
        (blow(50, false, true, false), true, None),
        (blow(10, false, false, false), true, None),
        (blow(10, false, false, true), false, None),
    ];
    let mut right = true;
    let mut said = Vec::new();
    for (blow, stopped, wanted) in rows {
        let attack = Side {
            heavy: blow.heavy,
            critical: blow.critical,
            lunge: blow.lunge,
            attacking: true,
            ..side(100, Guard::None)
        };
        let x = resolve(&attack, &side(blow.points, Guard::Held), false);
        let empty = x.stopped && matches!(x.took[1], Stagger | KnockedDown | Disarm);
        let got = empty.then_some(x.took[1]);
        let left = blow.points + x.points[1];
        let ok = x.stopped == stopped
            && got == wanted
            && (wanted.is_none() || left == meters::BROKEN_TO);
        right &= ok;
        said.push(format!(
            "{} Stamina, heavy {}, critical {}, lunge {}: stopped {}, blocker {:?}, left {left}{}",
            blow.points,
            blow.heavy,
            blow.critical,
            blow.lunge,
            x.stopped,
            x.took[1],
            if ok { "" } else { " (wrong)" }
        ));
    }
    check(right, said.join("; "))
}

/// What every exchange of a fight decided: the Stamina each side held, what
/// each played, whether the blow was stopped and how the blocker guarded.
type Decided = ([i32; 2], [Took; 2], bool, Guard, bool);

/// `text` played, with each of its exchanges.
fn played(text: &str) -> Result<(Vec<Snapshot>, Vec<Decided>), String> {
    tap::listen();
    let ticks = play(text);
    let decided = tap::take()
        .into_iter()
        .filter_map(|tapped| match tapped {
            Tapped::Exchange {
                points,
                took,
                stopped,
                guard,
                emptied,
                ..
            } => Some((points, took, stopped, guard, emptied)),
            _ => None,
        })
        .collect();
    tap::stop();
    Ok((ticks?, decided))
}

/// The same in a fight: seat 1 holds block with no Stamina against seat 0's
/// swings. The first blow its held block meets is stopped, and it is
/// staggered, knocked down or disarmed by it, and not hurt.
fn empty_block_in_the_world() -> Result<String, String> {
    let text = "scenario probe\nrules advanced\nseed 7\n\
                fighter 0 strong x=0 defence=2 tiers=2 fp=100 hp=400\n\
                fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
                begin\ntick 1 b0=attack b1=attack\ntick 60\n\
                duel 1 points=0\ntick 700 b0=attack h1=block\n";
    let (ticks, decided) = played(text)?;
    let first = decided.iter().find(|met| met.3 == Guard::Held);
    let hurt = ticks.iter().any(|tick| {
        tick.own(Seat(1))
            .and_then(|entity| entity.fighter)
            .is_some_and(|f| f.hp < 400 * 100)
    });
    check(
        first.is_some_and(|met| {
            met.2 && met.4 && matches!(met.1[1], Took::Stagger | Took::KnockedDown | Took::Disarm)
        }) && !hurt,
        format!(
            "the first blow the held block met at 0 Stamina: {first:?}; the blocker was hurt: {hurt}"
        ),
    )
}

/// A companion that leads the party (a portrait click) holds block when its
/// player's seat does, as the player's own character does: the blows that
/// meet it meet a held block, not a creature's guard, and are stopped.
fn driven_companion_blocks() -> Result<String, String> {
    let text = "scenario probe\nrules advanced\nseed 7\n\
                fighter 0 medium x=-300 defence=2 tiers=2 fp=100 hp=400\n\
                fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
                creature staff faction=friendly x=0 yaw=0 defence=2 tiers=2 hp=40000\n\
                begin\ntick 1 b1=attack\ntick 60\nlend c0\ntick 700 h0=block b1=attack\n";
    let (ticks, decided) = played(text)?;
    let companion = creature_fighters(&ticks);
    let led = companion.iter().any(|f| f.saber.duel.block_held);
    // Its double-bladed saber is in its main hand, as a companion's staff is
    // (`inventory::dress` puts the right-hand item in `weapons[0]`).
    let staff = companion
        .iter()
        .any(|f| f.saber.style == Some(crate::combat::saber::Style::Staff));
    let by = |guard: Guard| decided.iter().filter(|met| met.3 == guard).count();
    let stopped = decided.iter().any(|met| met.2 && met.3 == Guard::Held);
    check(
        led && staff && by(Guard::Held) > 0 && by(Guard::Npc) == 0 && stopped,
        format!(
            "the led staff companion (staff style {staff}) held block {led}; blows met by a held block {}, by a creature's guard {}, a held block stopped one {stopped}",
            by(Guard::Held),
            by(Guard::Npc)
        ),
    )
}

/// A critical hit on an empty block that cannot disarm the blocker (a blade
/// that is not a lightsaber: a vibroblade in a player's hand)
/// knocks the blocker down instead. Seeds are tried until the blow is a
/// critical hit; every blocker that took one went down.
fn refused_disarm_knocks_down() -> Result<String, String> {
    const SEEDS: u64 = 40;
    const SWINGS: usize = 6;
    let blocker = "fighter 1 vibroblade x=45 yaw=180 defence=2 tiers=2 hp=40000";
    let swing = "duel 1 points=0\ntick 120 b0=attack h1=block\n";
    let (mut criticals, mut missed, mut empties) = (0, 0, 0);
    for seed in 1..=SEEDS {
        let text = format!(
            "scenario probe\nrules advanced\nseed {seed}\n\
             fighter 0 strong x=0 defence=2 tiers=2 fp=100 hp=40000\n{blocker}\n\
             begin\ntick 1 b0=attack b1=attack\ntick 60\n{}",
            swing.repeat(SWINGS)
        );
        let (ticks, decided) = played(&text)?;
        empties += decided.iter().filter(|met| met.4).count();
        if !decided.iter().any(|met| met.4 && met.1[1] == Took::Disarm) {
            continue;
        }
        criticals += 1;
        let down = ticks.iter().any(|tick| {
            tick.own(Seat(1))
                .and_then(|entity| entity.fighter)
                .is_some_and(|f| f.saber.floored())
        });
        missed += usize::from(!down);
    }
    check(
        criticals > 0 && missed == 0,
        format!(
            "a player's vibroblade at 0 Stamina: {empties} empty blocks, {criticals} of {SEEDS} seeds had a critical hit on one, {missed} of those blockers not knocked down"
        ),
    )
}
