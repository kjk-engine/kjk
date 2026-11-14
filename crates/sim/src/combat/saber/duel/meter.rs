//! The Advanced rules' one bar (`docs/SABER-RULES.md` 1, 9.1): what a
//! fighter pays from (block points, which a player reads as Stamina; the
//! Force, where a rule names it, counted in points of 1% of the pool),
//! what a move costs, what fighting well gives back, the refill, and a
//! hilt's wear.

use super::Duel;
use super::standing;
use crate::combat::Fighter;
use crate::combat::saber::{Move, Style};
use rules::duel::costs;
use rules::duel::gains;
use rules::duel::refill;
use rules::duel::wear;

/// The Force left, in points (1% of the pool each).
pub fn force(f: &Fighter, pool: u16) -> i32 {
    (f.fp / i32::from(pool.max(1))).max(0)
}

/// What the meter holds: the block points.
pub fn meter(f: &Fighter) -> i32 {
    i32::from(f.saber.duel.points)
}

/// `points` of the Force gained, up to the pool (what is over it from a
/// power's temporary points is kept).
pub fn earn_force(f: &mut Fighter, pool: u16, points: i32) {
    let most = (i32::from(pool) * 100).max(f.fp);
    f.fp = (f.fp + points.max(0) * i32::from(pool)).min(most);
}

/// Block points moved by `points` (either way), within the bar: never
/// below 0 and never above `cap`, the most the holder's level allows
/// (`standing::cap`). A bar already over the cap, because the holder's
/// level fell or the save was made before the cap existed, is brought down
/// to it here.
pub fn shift_points(duel: &mut Duel, points: i32, cap: u8) {
    duel.points = (i32::from(duel.points) + points).clamp(0, i32::from(cap)) as u8;
}

/// `points` paid from a bar whose maximum is `cap`.
pub fn spend(f: &mut Fighter, points: i32, cap: u8) {
    if points <= 0 {
        return;
    }
    shift_points(&mut f.saber.duel, -points, cap);
}

/// `points` gained into a bar whose maximum is `cap`.
pub fn earn(f: &mut Fighter, points: i32, cap: u8) {
    shift_points(&mut f.saber.duel, points.max(0), cap);
}

/// A bar whose maximum is `cap` moved by `points`: paid when negative,
/// gained when positive.
pub fn change(f: &mut Fighter, points: i32, cap: u8) {
    if points < 0 {
        spend(f, -points, cap);
    } else {
        earn(f, points, cap);
    }
}

/// A swing begun (`next`, after `current`) and what it costs its fighter
/// (`docs/SABER-RULES.md` 1.2): `costs::SWING` block points, from a player
/// and a creature alike.
pub fn started(f: &mut Fighter, current: Move, next: Move, cap: u8) {
    if next != current && next.pure_attack() {
        spend(f, costs::SWING, cap);
    }
}

/// What a creature's own block move costs it (`docs/SABER-RULES.md` 1.2):
/// `costs::PAIR_KNOCKAWAY` for a knockaway with two blades,
/// `costs::PARRY` for a parry or a one-blade knockaway. A player pays
/// nothing for these, because the exchange already charged the block.
pub fn creature_blocked(f: &mut Fighter, style: Option<Style>, current: Move, next: Move, cap: u8) {
    if next == current {
        return;
    }
    let two_blades = matches!(style, Some(Style::Dual | Style::Staff));
    if next.knockaway() && two_blades {
        spend(f, costs::PAIR_KNOCKAWAY, cap);
    } else if next.knockaway() || next.parry() {
        spend(f, costs::PARRY, cap);
    }
}

/// How a fighter moves while its block points come back
/// (`refill::BY_STANCE`, in the order of that table).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stance {
    Crouched,
    #[default]
    Standing,
    Walking,
    Running,
}

impl Stance {
    /// How `ps` moves: running by its legs' animation, crouched by its
    /// duck, walking by its speed along the floor, else standing.
    pub fn of(ps: &crate::motion::MotionState) -> Stance {
        let [x, y, _] = ps.velocity;
        if crate::motion::anim::is_run(ps.legs.anim) {
            Stance::Running
        } else if ps.ducked() {
            Stance::Crouched
        } else if x * x + y * y > refill::WALKING_ABOVE * refill::WALKING_ABOVE {
            Stance::Walking
        } else {
            Stance::Standing
        }
    }

    /// How fast its block points come back, in percent of the standing
    /// rate (`refill::BY_STANCE`).
    pub fn percent(self) -> u64 {
        u64::from(refill::BY_STANCE[self as usize])
    }
}

/// What the refill reads of a saber user (`refill`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Rest {
    pub player: bool,
    /// The most block points its level lets it hold (`standing::cap`).
    pub cap: u8,
    /// Its Constitution modifier: how long it waits for block points to
    /// come back (`standing::refill_wait`).
    pub constitution: i32,
    pub on_ground: bool,
    pub stance: Stance,
    pub floored: bool,
    pub locked: bool,
    /// Swinging, in a special, a parry, a return, a stab, a bounce or a
    /// broken move.
    pub busy: bool,
    /// Its saber away from its hand.
    pub thrown: bool,
    /// Staggered or knocked aside, or just after a block.
    pub shaken: bool,
    /// In a rage's recovery.
    pub recovering: bool,
}

/// One step of a fighter's bar at `now` (`docs/SABER-RULES.md` 1.3, 9.1):
/// the duel's own tick (`refill::TICK_MS`: a crouching player's Force, the
/// Force a held block lets through, a busy creature's Force held); the
/// block points' refill; a hilt mending. Not in a scene
/// (`scene`). Every step holds the bar within what the fighter's level
/// allows, so one that stands over its maximum is brought down to it.
pub fn refill(f: &mut Fighter, pool: u16, rest: Rest, now: u64) {
    f.saber.duel.scened = false;
    shift_points(&mut f.saber.duel, 0, rest.cap);
    let calm = rest.on_ground && !rest.floored && !rest.busy && rest.stance != Stance::Running;
    if now >= f.saber.duel.tick_at {
        f.saber.duel.tick_at = now + refill::TICK_MS;
        if rest.player && rest.stance == Stance::Crouched && calm {
            earn_force(f, pool, refill::CROUCH);
        }
        if rest.player && f.saber.duel.block_held && now >= f.saber.duel.force_at {
            earn_force(f, pool, refill::GAIN);
            f.saber.duel.force_at = now + refill::FORCE_HOLD_MS;
        }
        if !rest.player && rest.thrown {
            let duel = &mut f.saber.duel;
            duel.force_at = duel.force_at.max(now + refill::CREATURE_BUSY_MS);
        }
    }
    points(f, rest, now);
    mend(&mut f.saber.duel, now);
}

/// A step of a fighter in a scene, which plays by KOTOR's rules under every
/// saber mode (`world::scene::Held::saber_rules`): its bar stands still,
/// and a player's is filled as the scene starts, as Movie Duels sets it at
/// a cutscene's start (`docs/SABER-RULES.md` 1.3). Its Force is KOTOR's and
/// is left as it is: a conversation refills no one's.
pub fn scene(f: &mut Fighter, player: bool, cap: u8) {
    let duel = &mut f.saber.duel;
    if player && !duel.scened {
        duel.points = cap;
    }
    duel.scened = true;
    duel.warn = false;
}

/// The block points' refill (`docs/SABER-RULES.md` 1.3), the same for a
/// player and a creature: `refill::GAIN` points every `refill::TICK_MS`
/// scaled by how the fighter moves (`Stance::percent`: more crouching,
/// less walking, none running), unless it is spending them: swinging,
/// shaken, down, off the ground or, for a player, holding block. A thrown
/// saber, or a rage's recovery, makes each wait longer unless crouching.
/// Every wait is shortened or lengthened by the fighter's Constitution
/// modifier (`wait_of`). The next point is due one wait after the last was
/// due, so that a wait shorter than a step is kept on average.
fn points(f: &mut Fighter, rest: Rest, now: u64) {
    let duel = &mut f.saber.duel;
    let percent = rest.stance.percent();
    let spending = rest.busy
        || rest.shaken
        || rest.floored
        || !rest.on_ground
        || (rest.player && duel.block_held);
    if now < duel.refill_at || percent == 0 || spending {
        return;
    }
    shift_points(duel, refill::GAIN, rest.cap);
    let mut wait = refill::TICK_MS * 100 / percent;
    if rest.stance != Stance::Crouched {
        if rest.thrown {
            wait += refill::SLOW_MS;
        } else if rest.recovering {
            wait += refill::TICK_MS;
        }
    }
    let wait = wait_of(wait, rest.constitution).max(1);
    let kept = duel.refill_at + wait;
    duel.refill_at = if kept > now { kept } else { now + wait };
}

/// A refill wait of `wait_ms` as a fighter whose Constitution modifier is
/// `constitution` waits it (`standing::refill_wait`, thousandths).
fn wait_of(wait_ms: u64, constitution: i32) -> u64 {
    wait_ms * standing::refill_wait(constitution) as u64 / 1000
}

/// A hilt mending (`wear::MEND_MS`), never back to cracked's mark once
/// past it.
fn mend(duel: &mut Duel, now: u64) {
    let [after, every] = wear::MEND_MS;
    if duel.wear == 0 || duel.broken || now < duel.wear_at + after {
        return;
    }
    let floor = if duel.wear > wear::LEVELS[0] {
        wear::LEVELS[0] + 1
    } else {
        0
    };
    duel.wear = duel.wear.saturating_sub(1).max(floor);
    duel.wear_at = now + every - after;
}

/// What a blow on another gives its striker (`docs/SABER-RULES.md` 1.3): a
/// kill `gains::KILL` (`gains::KILL_BLOCKING` to a player holding block);
/// a player's hit at the easier difficulties `gains::HURT` or a killing
/// one `gains::HURT_LETHAL` (`gains::HURT_BLOCKING` holding block).
pub fn rewarded(f: &mut Fighter, player: bool, killed: bool, cap: u8) {
    let blocking = player && f.saber.duel.block_held;
    let mut gain = 0;
    if killed {
        gain += if blocking {
            gains::KILL_BLOCKING
        } else {
            gains::KILL
        };
    }
    if player && rules::duel::difficulty::LEVEL <= gains::HURT_EASIEST {
        gain += match (blocking, killed) {
            (true, _) => gains::HURT_BLOCKING,
            (false, true) => gains::HURT_LETHAL,
            (false, false) => gains::HURT,
        };
    }
    earn(f, gain, cap);
}

/// Whose hilts wear (`wear::WHO`): nobody's, creatures', everyone's.
pub fn wears(player: bool) -> bool {
    match wear::WHO {
        0 => false,
        1 => !player,
        _ => true,
    }
}

/// What a blocked blow wears a hilt (`docs/SABER-RULES.md` 9.1): by the
/// attacker's style or move, more from a raging attacker and on a spent
/// blocker; rounded up.
pub fn wear_of(style: Style, special: bool, raging: bool, blocker_points: i32) -> u16 {
    let [heavy, medium, other] = wear::GAINS;
    let base = match style {
        _ if special || super::heavy(style) => heavy,
        Style::Medium => medium,
        _ => other,
    };
    let mut worn = f32::from(base);
    if raging {
        worn *= wear::TIMES[0];
    }
    if blocker_points <= wear::LOW {
        worn *= wear::TIMES[1];
    }
    worn.ceil() as u16
}

/// A hilt worn by `worn` at `now`; whether it is worn through
/// (`wear::LEVELS`).
pub fn wear(duel: &mut Duel, worn: u16, now: u64) -> bool {
    duel.wear = duel.wear.saturating_add(worn);
    duel.wear_at = now;
    duel.wear >= wear::LEVELS[1]
}
