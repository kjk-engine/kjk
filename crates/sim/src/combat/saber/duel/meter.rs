//! The duel modes' meters (`docs/SABER-RULES.md` 1, 5, 9.1): what a saber
//! user pays from (the Force pool under MD, counted in points of 1% of the
//! pool; block points under MDA), what a move costs, what fighting well
//! gives back, the refill, the mishap count's rises and falls, fatigue,
//! and a hilt's wear.

use super::{Duel, SaberRules};
use crate::combat::Fighter;
use crate::combat::saber::{Move, Style};
use rules::duel::costs;
use rules::duel::gains;
use rules::duel::master;
use rules::duel::meters;
use rules::duel::mishap;
use rules::duel::refill;
use rules::duel::sprint;
use rules::duel::wear;

/// The Force left, in points (1% of the pool each).
pub fn force(f: &Fighter, pool: u16) -> i32 {
    (f.fp / i32::from(pool.max(1))).max(0)
}

/// What the meter holds: block points under MDA, the Force under MD.
pub fn meter(rules: SaberRules, f: &Fighter, pool: u16) -> i32 {
    if rules.own_meter() {
        i32::from(f.saber.duel.points)
    } else {
        force(f, pool)
    }
}

/// `points` of the Force spent (none below empty).
pub fn spend_force(f: &mut Fighter, pool: u16, points: i32) {
    f.fp = (f.fp - points.max(0) * i32::from(pool)).max(0);
}

/// `points` of the Force gained, up to the pool (what is over it from a
/// power's temporary points is kept).
pub fn earn_force(f: &mut Fighter, pool: u16, points: i32) {
    let most = (i32::from(pool) * 100).max(f.fp);
    f.fp = (f.fp + points.max(0) * i32::from(pool)).min(most);
}

/// Block points moved by `points` (either way), within the meter.
pub fn shift_points(duel: &mut Duel, points: i32) {
    let most = i32::from(meters::POINTS_MAX);
    duel.points = (i32::from(duel.points) + points).clamp(0, most) as u8;
}

/// `points` paid from the meter; a cost that leaves it low fatigues
/// (`meters::FATIGUED_DRAIN`) or drains (`meters::DRAINED`) its payer.
pub fn spend(rules: SaberRules, f: &mut Fighter, pool: u16, points: i32) {
    if points <= 0 {
        return;
    }
    if rules.own_meter() {
        shift_points(&mut f.saber.duel, -points);
    } else {
        spend_force(f, pool, points);
    }
    let left = meter(rules, f, pool);
    let duel = &mut f.saber.duel;
    duel.fatigued |= left <= meters::FATIGUED_DRAIN;
    duel.drained |= left < meters::DRAINED;
}

/// `points` gained into the meter.
pub fn earn(rules: SaberRules, f: &mut Fighter, pool: u16, points: i32) {
    if rules.own_meter() {
        shift_points(&mut f.saber.duel, points.max(0));
    } else {
        earn_force(f, pool, points);
    }
}

/// A meter moved by `points`: paid when negative, gained when positive.
pub fn change(rules: SaberRules, f: &mut Fighter, pool: u16, points: i32) {
    if points < 0 {
        spend(rules, f, pool, -points);
    } else {
        earn(rules, f, pool, points);
    }
}

/// What a blocker pays for `cost` under `rules`: a master under MDA its
/// share (`master::SABER_SHARE`, `master::BOLT_SHARE` against a bolt),
/// rounded, at least a point.
pub fn price(rules: SaberRules, is_master: bool, cost: i32, bolt: bool) -> i32 {
    if cost <= 0 || !rules.own_meter() || !is_master {
        return cost;
    }
    let share = if bolt {
        master::BOLT_SHARE
    } else {
        master::SABER_SHARE
    };
    ((cost * share + 50) / 100).max(1)
}

/// The mishap count moved by `count` (a rise when positive): running
/// (`mishap::RUN_RISE`) multiplies a rise and divides a fall; within
/// `mishap::MAX`.
pub fn shift(duel: &mut Duel, count: i32, running: bool) {
    let count = match (count > 0, running) {
        (true, true) => count * mishap::RUN_RISE,
        (false, true) => count / mishap::RUN_RISE,
        _ => count,
    };
    duel.mishap = (i32::from(duel.mishap) + count).clamp(0, i32::from(mishap::MAX)) as u8;
}

/// A move begun (`next`, after `current`) and what it costs
/// (`docs/SABER-RULES.md` 1.2): every swing a mishap, a player's plain
/// swing a point; a creature's knockaway with two blades, under MDA its
/// parries and one-blade knockaways, under MD its kicks.
#[allow(clippy::too_many_arguments)]
pub fn started(
    rules: SaberRules,
    f: &mut Fighter,
    pool: u16,
    player: bool,
    style: Option<Style>,
    current: Move,
    next: Move,
    running: bool,
) {
    if next == current {
        return;
    }
    if next.attack() {
        shift(&mut f.saber.duel, mishap::SWING, running);
        if player && next.pure_attack() {
            spend(rules, f, pool, costs::SWING);
        }
    }
    if player {
        return;
    }
    let two_blades = matches!(style, Some(Style::Dual | Style::Staff));
    if next.knockaway() && two_blades {
        spend(rules, f, pool, costs::PAIR_KNOCKAWAY);
    } else if rules.own_meter() && (next.knockaway() || next.parry()) {
        spend(rules, f, pool, costs::PARRY);
    } else if !rules.own_meter() && next.kick() {
        spend_force(f, pool, costs::KICK);
    }
}

/// The marks the meters set (`docs/SABER-RULES.md` 1.4): fatigued while
/// the Force is at or under `meters::FATIGUED`; under MDA drained while
/// the block points are under `meters::DRAINED_POINTS`, under MD no more
/// once the meter is back to `meters::DRAINED`.
pub fn settle(rules: SaberRules, f: &mut Fighter, pool: u16) {
    let (force, left) = (force(f, pool), meter(rules, f, pool));
    let duel = &mut f.saber.duel;
    duel.fatigued |= force <= meters::FATIGUED;
    if rules.own_meter() {
        let points = i32::from(duel.points);
        if points < meters::DRAINED_POINTS {
            duel.drained = true;
        } else if points > meters::DRAINED_POINTS {
            duel.drained = false;
        }
    } else if left >= meters::DRAINED {
        duel.drained = false;
    }
}

/// What the refill reads of a saber user (`refill`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Rest {
    pub player: bool,
    pub master: bool,
    /// Its blade lit in hand.
    pub lit: bool,
    pub on_ground: bool,
    pub running: bool,
    pub crouched: bool,
    pub floored: bool,
    pub locked: bool,
    /// Swinging, in a special, a parry, a return, a stab, a bounce or a
    /// broken move.
    pub busy: bool,
    /// Its weapon's timer out.
    pub idle: bool,
    /// Its saber away from its hand.
    pub thrown: bool,
    /// Staggered or knocked aside, or just after a block.
    pub shaken: bool,
    /// In a rage's recovery.
    pub recovering: bool,
    /// In a scene.
    pub scene: bool,
}

/// One step of a saber user's duel meters at `now` (`docs/SABER-RULES.md`
/// 1.3 to 1.4, 2.4, 5.5, 9.1): the marks; the duel's own tick
/// (`refill::TICK_MS`: fatigue lifted, a crouching player's extra Force
/// and calm, the Force a held block lets through, a busy creature's Force
/// held); the block points' refill; the mishaps' decay; a hilt mending;
/// the sprint's fuel; a scene's refill.
pub fn refill(rules: SaberRules, f: &mut Fighter, pool: u16, rest: Rest, now: u64) {
    settle(rules, f, pool);
    if rest.player && rest.scene && !f.saber.duel.scened {
        f.saber.duel.points = meters::POINTS_MAX;
        earn_force(f, pool, i32::from(meters::POINTS_MAX));
        f.saber.duel.mishap = mishap::SCENE;
    }
    f.saber.duel.scened = rest.scene;
    let calm = rest.on_ground && !rest.floored && !rest.busy && !rest.running;
    if now >= f.saber.duel.tick_at {
        f.saber.duel.tick_at = now + refill::TICK_MS;
        if force(f, pool) > meters::FATIGUED {
            f.saber.duel.fatigued = false;
        }
        if rest.player && rest.crouched && calm {
            earn_force(f, pool, refill::CROUCH);
            shift(&mut f.saber.duel, -refill::GAIN, false);
        }
        if rest.player && f.saber.duel.block_held && now >= f.saber.duel.force_at {
            earn_force(f, pool, refill::GAIN);
            f.saber.duel.force_at = now + rules.of(refill::FORCE_HOLD_MS);
        }
        let held = if rules.own_meter() {
            rest.thrown
        } else {
            rest.busy || rest.thrown || (rest.running && rest.lit)
        };
        if !rest.player && held {
            let duel = &mut f.saber.duel;
            duel.force_at = duel.force_at.max(now + refill::CREATURE_BUSY_MS);
        }
    }
    points(rules, f, pool, rest, calm, now);
    decay(f, rest, now);
    mend(&mut f.saber.duel, now);
    fuel(&mut f.saber.duel, rest, now);
}

/// The block points' refill (`docs/SABER-RULES.md` 1.3): a player's a
/// point a tick while it walks or stands free, not holding block (more
/// crouching, a wait after it unless crouching); a creature's
/// `refill::CREATURE_GAIN` every `refill::CREATURE_EVERY_MS` (and as much Force
/// under MD) unless shaken.
fn points(rules: SaberRules, f: &mut Fighter, pool: u16, rest: Rest, calm: bool, now: u64) {
    if now < f.saber.duel.refill_at {
        return;
    }
    if !rest.player {
        if rest.shaken || rest.floored {
            return;
        }
        f.saber.duel.refill_at = now + refill::CREATURE_EVERY_MS;
        shift_points(&mut f.saber.duel, refill::CREATURE_GAIN);
        if !rules.own_meter() && rest.lit {
            earn_force(f, pool, refill::CREATURE_GAIN);
        }
        return;
    }
    let duel = &mut f.saber.duel;
    if !calm || duel.block_held {
        return;
    }
    let crouching = rest.crouched;
    shift_points(
        duel,
        refill::GAIN + if crouching { refill::CROUCH } else { 0 },
    );
    let mut wait = refill::TICK_MS;
    if !crouching {
        if rest.recovering {
            wait += refill::TICK_MS;
        } else if i32::from(duel.points) > refill::SLOW_ABOVE || rest.thrown || rest.idle {
            wait += refill::SLOW_MS;
        }
    }
    if rest.master {
        wait = wait * master::WAIT_SHARE / 100;
    }
    duel.refill_at = now + wait;
}

/// The mishaps' fall (`docs/SABER-RULES.md` 5.5): every `mishap::DECAY_MS`
/// while free on the ground, not just after a block and (a player) not
/// holding block: faster for a player above `mishap::FAST_ABOVE`.
fn decay(f: &mut Fighter, rest: Rest, now: u64) {
    let duel = &mut f.saber.duel;
    if now < duel.decay_at {
        return;
    }
    duel.decay_at = now + mishap::DECAY_MS;
    let free = rest.on_ground && !rest.floored && !rest.busy && !rest.shaken && !rest.locked;
    if !free || (rest.player && duel.block_held) {
        return;
    }
    let fast = rest.player && duel.mishap > mishap::FAST_ABOVE;
    shift(duel, -mishap::DECAY[usize::from(fast)], false);
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

/// The sprint's fuel (`docs/SABER-RULES.md` 2.4): burnt while sprinting,
/// back otherwise (quicker crouching, slower while low).
fn fuel(duel: &mut Duel, rest: Rest, now: u64) {
    if now < duel.fuel_at {
        return;
    }
    let [full, low, ..] = sprint::FUEL;
    let [burn, back, crouched, slow] = sprint::EVERY_MS;
    if duel.sprint {
        duel.fuel = duel.fuel.saturating_sub(1);
        duel.fuel_at = now + burn;
    } else {
        duel.fuel = (duel.fuel + 1).min(full);
        duel.fuel_at = now
            + match () {
                _ if duel.fuel < low => slow,
                _ if rest.crouched => crouched,
                _ => back,
            };
    }
}

/// A sprint begun: its start's cost when the fuel is low.
pub fn sprint_started(duel: &mut Duel) {
    let [_, _, start_below, cost] = sprint::FUEL;
    if duel.fuel < start_below {
        duel.fuel = duel.fuel.saturating_sub(cost);
    }
}

/// What a blow on another gives its striker (`docs/SABER-RULES.md` 1.3): a
/// kill `gains::KILL` (`gains::KILL_BLOCKING` to a player holding block);
/// a player's hit at the easier difficulties `gains::HURT` or a killing
/// one `gains::HURT_LETHAL` (`gains::HURT_BLOCKING` holding block). Either
/// calms a high count to `mishap::RESET`'s mark, not a player's holding
/// block.
pub fn rewarded(rules: SaberRules, f: &mut Fighter, pool: u16, player: bool, killed: bool) {
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
    if gain == 0 {
        return;
    }
    earn(rules, f, pool, gain);
    let [from, to] = mishap::RESET;
    if !blocking && f.saber.duel.mishap >= from {
        f.saber.duel.mishap = to;
    }
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
