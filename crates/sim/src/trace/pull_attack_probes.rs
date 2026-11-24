//! The probes of the pull stab and the pull swing (`kjk sim probes --only
//! "pull attack"`): a character who presses [Attack] while the hand of a
//! Force Pull is still raised attacks the enemy in front with its blade.
//! - The fast style stabs an enemy within the stab's reach, and pays for it.
//! - The medium style swings at an enemy between the swing's two reaches.
//! - The swing is not made at an enemy nearer than its near reach, and the
//!   stab is not made at one beyond its reach.
//! - Without a raised Pull hand, the same press makes an ordinary swing.
//! - Once the Pull hand has come down, the press makes an ordinary swing.
//!
//! Each is played from a trace scenario (`trace::duels`) under Jedi
//! Academy's rules.
use super::duels::play;
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::saber::Move;
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;
use rules::saber_moves::specials;
/// Ticks of the settling, in which the blades are lit, and the wait after
/// it before a weapon is ready.
const SETTLED: usize = 61;
const READY: usize = 150;
/// How many milliseconds a tick lasts.
const TICK_MS: usize = 8;
/// How many ticks the key is held after the hand is raised.
const HELD: usize = 20;
/// The Force points the fighter starts with.
const FP: u16 = 100;
/// How far along the arena's y axis (units) the fighters stand, clear of its
/// steps and its block.
const LANE: i32 = 250;
/// How far (units) the foe stands from the fighter beyond what its reach
/// asks, or within it: a margin that keeps the foe clear of each edge.
const MARGIN: f32 = 40.0;
/// A pull of seat 0, a `style` fighter, at a foe `away` units off, with the
/// Pull hand raised for `hand_ms` before the attack key is held. What seat 0's
/// fighter did through the ticks after the hand was raised.
fn pulled(style: &str, away: f32, hand_ms: usize, wait: usize) -> Result<Vec<Fighter>, String> {
    let hand = if hand_ms > 0 {
        format!("force 0 pull_hand={hand_ms}\n")
    } else {
        String::new()
    };
    let waiting = if wait > 0 {
        format!("tick {wait}\n")
    } else {
        String::new()
    };
    let text = format!(
        "scenario pull attack\nrules ja\nseed 7\n\
         fighter 0 {style} x=0 y={LANE} defence=2 tiers=2 fp={FP} hp=400\n\
         fighter 1 medium x={away} y={LANE} yaw=180 defence=2 tiers=2 fp=100 hp=4000\n\
         sheet 0 level=5\nbegin\nsaber 0 style={style}\ntick 1 b0=attack b1=attack\ntick {}\n\
         tick {READY}\n{hand}{waiting}tick {HELD} b0=attack\ntick 10\n",
        SETTLED - 1,
    );
    let ticks: Vec<Snapshot> = play(&text)?;
    let from = SETTLED + READY;
    Ok(ticks
        .iter()
        .skip(from)
        .filter_map(|tick| tick.own(Seat(0)).and_then(|entity| entity.fighter))
        .collect())
}
/// Whether the fighter played `attack` at any tick.
fn made(fighters: &[Fighter], attack: Move) -> bool {
    fighters
        .iter()
        .any(|fighter| fighter.saber.current_move == attack)
}
/// Whether the fighter played either pull attack at any tick.
fn pulled_attack(fighters: &[Fighter]) -> bool {
    made(fighters, Move::PULL_ATTACK_STAB) || made(fighters, Move::PULL_ATTACK_SWING)
}
/// The Force points the fighter lost over the ticks.
fn paid(fighters: &[Fighter]) -> i32 {
    let first = i32::from(FP) * 100;
    let lowest = fighters.iter().map(|f| f.fp).min().unwrap_or(0);
    first - lowest
}
fn stab_in_reach() -> Result<String, String> {
    let away = specials::PULL_STAB_REACH - MARGIN;
    let fighters = pulled("fast", away, 400, 0)?;
    check(
        made(&fighters, Move::PULL_ATTACK_STAB) && paid(&fighters) > 0,
        format!(
            "fast style, foe {away} units off: pull stab {}, {} hundredths of Force paid",
            made(&fighters, Move::PULL_ATTACK_STAB),
            paid(&fighters)
        ),
    )
}
fn swing_between_reaches() -> Result<String, String> {
    let [near, far] = specials::PULL_SWING_REACH;
    let away = (near + far) / 2.0;
    let fighters = pulled("medium", away, 400, 0)?;
    check(
        made(&fighters, Move::PULL_ATTACK_SWING) && paid(&fighters) > 0,
        format!(
            "medium style, foe {away} units off: pull swing {}, {} hundredths of Force paid",
            made(&fighters, Move::PULL_ATTACK_SWING),
            paid(&fighters)
        ),
    )
}
fn swing_too_near() -> Result<String, String> {
    let [near, _] = specials::PULL_SWING_REACH;
    let away = near - MARGIN;
    let fighters = pulled("medium", away, 400, 0)?;
    check(
        !pulled_attack(&fighters),
        format!(
            "medium style, foe {away} units off: pull attack made: {}",
            pulled_attack(&fighters)
        ),
    )
}
fn stab_too_far() -> Result<String, String> {
    let away = specials::PULL_STAB_REACH + MARGIN;
    let fighters = pulled("fast", away, 400, 0)?;
    check(
        !pulled_attack(&fighters),
        format!(
            "fast style, foe {away} units off: pull attack made: {}",
            pulled_attack(&fighters)
        ),
    )
}
fn no_pull_no_attack() -> Result<String, String> {
    let away = specials::PULL_STAB_REACH - MARGIN;
    let fighters = pulled("fast", away, 0, 0)?;
    check(
        !pulled_attack(&fighters),
        format!(
            "fast style without a pull: pull attack made: {}",
            pulled_attack(&fighters)
        ),
    )
}
fn hand_down_no_attack() -> Result<String, String> {
    let away = specials::PULL_STAB_REACH - MARGIN;
    // The key is first pressed after the hand has come down.
    let wait = 400 / TICK_MS + 10;
    let fighters = pulled("fast", away, 400, wait)?;
    check(
        !pulled_attack(&fighters),
        format!(
            "fast style, key pressed {wait} ticks after the pull: pull attack made: {}",
            pulled_attack(&fighters)
        ),
    )
}
pub fn probes() -> Checks {
    vec![
        ("the fast style stabs after a pull", stab_in_reach()),
        (
            "the medium style swings after a pull",
            swing_between_reaches(),
        ),
        ("no pull swing at a near enemy", swing_too_near()),
        ("no pull stab at a far enemy", stab_too_far()),
        ("no pull attack without a pull", no_pull_no_attack()),
        (
            "no pull attack after the hand is down",
            hand_down_no_attack(),
        ),
    ]
}
