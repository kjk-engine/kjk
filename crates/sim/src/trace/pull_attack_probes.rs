//! The probes of the pull stab and the pull swing (`kjk sim probes --only
//! "pull attack"`): a character who presses [Attack] within the window of a
//! Force Pull attacks the enemy in front with its blade.
//! - The fast style stabs an enemy within the stab's reach, and pays for it.
//! - The medium style swings at an enemy within the swing's reach, however
//!   close, and pays for it.
//! - Neither the swing nor the stab is made at an enemy beyond its reach.
//! - Both swipes of the pull swing deal damage, not only the second.
//! - Without a pull, the same press makes an ordinary swing.
//! - After the pull's hand has come down, the window is still open.
//! - Once the window has closed, the press makes an ordinary swing.
//! - A key held down through the pull makes no pull attack. The attack
//!   needs a fresh press.
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
/// How many ticks the key is up after the pull before it is pressed. One
/// tick makes the press a fresh one.
const FRESH: usize = 1;
/// The Force points the fighter starts with.
const FP: u16 = 100;
/// The window of a pull (ms).
const WINDOW: u64 = specials::PULL_WINDOW_MS;
/// How far along the arena's y axis (units) the fighters stand, clear of its
/// steps and its block.
const LANE: i32 = 250;
/// How far (units) the foe stands from the fighter beyond what its reach
/// asks, or within it: a margin that keeps the foe clear of each edge.
const MARGIN: f32 = 40.0;
/// How far (units) a foe stands in melee range, the bodies almost touching.
const MELEE: f32 = 60.0;
/// A pull of seat 0, a `style` fighter, at a foe `away` units off, with the
/// window open for `window_ms` and the Pull hand raised for `hand_ms` (neither
/// if both are 0), and the attack key held after `wait` ticks. What seat 0's
/// fighter did through the ticks after the pull.
fn pulled(
    style: &str,
    away: f32,
    (window_ms, hand_ms): (u64, usize),
    wait: usize,
) -> Result<Vec<Fighter>, String> {
    let hand = match (window_ms, hand_ms) {
        (0, 0) => String::new(),
        (_, 0) => format!("force 0 pull_window={window_ms}\n"),
        _ => format!("force 0 pull_window={window_ms} pull_hand={hand_ms}\n"),
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
    let fighters = pulled("fast", away, (WINDOW, 400), FRESH)?;
    check(
        made(&fighters, Move::PULL_ATTACK_STAB) && paid(&fighters) > 0,
        format!(
            "fast style, foe {away} units off: pull stab {}, {} hundredths of Force paid",
            made(&fighters, Move::PULL_ATTACK_STAB),
            paid(&fighters)
        ),
    )
}
/// The medium style's swing at a foe `away` units off: whether the pull swing
/// was made, and the Force paid (hundredths).
fn swung(away: f32) -> Result<(bool, i32), String> {
    let fighters = pulled("medium", away, (WINDOW, 400), FRESH)?;
    Ok((made(&fighters, Move::PULL_ATTACK_SWING), paid(&fighters)))
}
fn swing_in_reach() -> Result<String, String> {
    let away = specials::PULL_SWING_REACH - MARGIN;
    let (swing, paid) = swung(away)?;
    check(
        swing && paid > 0,
        format!("medium style, foe {away} units off: pull swing {swing}, {paid} Force paid"),
    )
}
fn swing_in_melee_range() -> Result<String, String> {
    let (swing, paid) = swung(MELEE)?;
    check(
        swing && paid > 0,
        format!("medium style, foe {MELEE} units off: pull swing {swing}, {paid} Force paid"),
    )
}
fn swing_too_far() -> Result<String, String> {
    let away = specials::PULL_SWING_REACH + MARGIN;
    let (swing, _) = swung(away)?;
    check(
        !swing,
        format!("medium style, foe {away} units off: pull swing made: {swing}"),
    )
}
fn stab_too_far() -> Result<String, String> {
    let away = specials::PULL_STAB_REACH + MARGIN;
    let fighters = pulled("fast", away, (WINDOW, 400), FRESH)?;
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
    let fighters = pulled("fast", away, (0, 0), FRESH)?;
    check(
        !pulled_attack(&fighters),
        format!(
            "fast style without a pull: pull attack made: {}",
            pulled_attack(&fighters)
        ),
    )
}
fn window_closed_no_attack() -> Result<String, String> {
    let away = specials::PULL_STAB_REACH - MARGIN;
    // The key is first pressed after the window has closed.
    let wait = WINDOW as usize / TICK_MS + 10;
    let fighters = pulled("fast", away, (WINDOW, 0), wait)?;
    check(
        !pulled_attack(&fighters),
        format!(
            "fast style, key pressed {wait} ticks after the pull: pull attack made: {}",
            pulled_attack(&fighters)
        ),
    )
}
fn stab_with_hand_down() -> Result<String, String> {
    let away = specials::PULL_STAB_REACH - MARGIN;
    // The key is pressed after the pull's hand has come down, inside the window.
    let wait = (specials::PULL_WINDOW_MS as usize / 2) / TICK_MS;
    let fighters = pulled("fast", away, (WINDOW, 0), wait)?;
    check(
        made(&fighters, Move::PULL_ATTACK_STAB),
        format!(
            "fast style, key pressed {wait} ticks after the pull: pull stab {}",
            made(&fighters, Move::PULL_ATTACK_STAB)
        ),
    )
}
pub fn probes() -> Checks {
    vec![
        ("the fast style stabs after a pull", stab_in_reach()),
        ("the medium style swings after a pull", swing_in_reach()),
        (
            "the medium style swings at an enemy in melee range",
            swing_in_melee_range(),
        ),
        ("no pull swing at a far enemy", swing_too_far()),
        ("no pull stab at a far enemy", stab_too_far()),
        ("no pull attack without a pull", no_pull_no_attack()),
        ("both swipes of the pull swing hit", both_swipes_hit()),
        (
            "the window is open with the hand down",
            stab_with_hand_down(),
        ),
        (
            "no pull attack after the window closes",
            window_closed_no_attack(),
        ),
        (
            "no pull attack with [Attack] held through the pull",
            held_through_no_attack(),
        ),
    ]
}
fn held_through_no_attack() -> Result<String, String> {
    let away = specials::PULL_STAB_REACH - MARGIN;
    // The key is down on the first tick of the window, as if it was held
    // while the pull was cast.
    let fighters = pulled("fast", away, (WINDOW, 400), 0)?;
    check(
        !pulled_attack(&fighters),
        format!(
            "fast style, key held through the pull: pull attack made: {}",
            pulled_attack(&fighters)
        ),
    )
}
/// The pull swing deals damage through its first swipe as well as its
/// second: from the very start of the move, and not in its last
/// `PULL_SWING` milliseconds. Played by the stand-in animation table, whose
/// every animation is a second long.
fn both_swipes_hit() -> Result<String, String> {
    use crate::combat::saber::power;
    use crate::motion::{Anim, AnimTable};
    let anims = AnimTable::stand_in();
    let anim = Anim::BOTH_PULL_IMPALE_SWING;
    let length = anims.get(anim).length_ms();
    let [last, first] = rules::sabers::windows::pull::PULL_SWING;
    let at = |elapsed: i32| power::animation(&anims, anim, length - elapsed, false);
    let (start, early, late) = (at(first), at(first + 100), at(length - last + 100));
    check(
        first == 0 && start > 0 && early > 0 && late == 0,
        format!(
            "power at the start {start}, in the first swipe {early}, in the last {last} ms {late}; \
             the window starts after {first} ms"
        ),
    )
}
