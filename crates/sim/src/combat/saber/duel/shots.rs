//! Bolts against the blade under a duel mode (`docs/SABER-RULES.md` 7):
//! what turning one costs and where it goes. Who blocks is the world's
//! (`world::sabers::bolt_block`).

use crate::math::{self, Vec3};
use crate::rng::SimRng;
use rules::duel::bolts;

/// The blocker as a deflection's cost reads it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Blocker {
    pub player: bool,
    pub speeding: bool,
    pub running: bool,
    pub airborne: bool,
    /// Walking and moving.
    pub walking: bool,
    pub broken: bool,
    pub kicking: bool,
    /// Two sabers.
    pub pair: bool,
    /// Block and attack held `perfect::HELD_LONG_MS` or more.
    pub held_long: bool,
}

/// What turning a bolt costs (`docs/SABER-RULES.md` 7.2): the base, more
/// by how the blocker moves, then multiplied; twice that aimed back at its
/// shooter; rounded down.
pub fn cost(blocker: &Blocker, aimed: bool) -> i32 {
    let [speed, run, air, walk] = bolts::ADDS;
    let [broken, kicking, running, airborne, air_pair, held] = bolts::TIMES;
    let mut cost = bolts::BASE;
    for (on, add) in [
        (blocker.speeding, speed),
        (blocker.running, run),
        (blocker.airborne, air),
        (blocker.walking, walk),
    ] {
        if on {
            cost += add;
        }
    }
    for (on, times) in [
        (blocker.broken, broken),
        (blocker.kicking, kicking),
        (blocker.running && blocker.player, running),
        (blocker.airborne && !blocker.pair, airborne),
        (blocker.airborne && blocker.pair, air_pair),
        (blocker.held_long, held),
    ] {
        if on {
            cost *= times;
        }
    }
    if aimed {
        cost *= bolts::AIMED;
    }
    cost as i32
}

/// What a bolt that arrives exactly on the edge of an arc may fall short of
/// it by, from the rounding of the two unit vectors.
const EDGE_SLACK: f32 = 8.0 * f32::EPSILON;

/// Whether a shooter in the way `from_shooter` (from the blocker, any
/// length) lies within the arc the Jedi Defense feat at `tier` covers either
/// side of `facing`, both seen from above. The edge is inside. A tier of 0
/// covers nothing, and a shooter straight above or below has no way across
/// the floor, so it is outside.
pub fn in_arc(tier: u8, facing: Vec3, from_shooter: Vec3) -> bool {
    let Some(&cos) = usize::from(tier)
        .checked_sub(1)
        .and_then(|row| bolts::ARC_COS.get(row.min(bolts::ARC_COS.len() - 1)))
    else {
        return false;
    };
    let flat = |mut way: Vec3| {
        way[2] = 0.0;
        (math::normalize(&mut way) > 0.0).then_some(way)
    };
    match (flat(facing), flat(from_shooter)) {
        (Some(facing), Some(way)) => math::dot(facing, way) >= cos - EDGE_SLACK,
        _ => false,
    }
}

/// The way from the blocker at `origin` to the shooter of a bolt flying at
/// `velocity`: toward the shooter's place when it is known, else back along
/// the way the bolt came.
pub fn to_shooter(origin: Vec3, shooter: Option<Vec3>, velocity: Vec3) -> Vec3 {
    shooter.map_or_else(|| math::scale(velocity, -1.0), |at| math::sub(at, origin))
}

/// What a deflected bolt takes of a blocker whose Jedi Defense feat stands
/// at `tier` and whose weapon's deflection bonus is `deflect`, when the
/// bolt's normal cost is `cost`: the cost less `bolts::TIER_OFF` percent
/// for each tier and `bolts::DEFLECT_OFF` percent for each point of the
/// bonus, never under `bolts::PRICE_LEAST` percent of it, rounded, and at
/// least 1.
pub fn bolt_price(cost: i32, tier: u8, deflect: i8) -> i32 {
    let percent =
        (100 - bolts::TIER_OFF * i32::from(tier) - bolts::DEFLECT_OFF * i32::from(deflect))
            .max(bolts::PRICE_LEAST);
    ((cost * percent + 50) / 100).max(1)
}

/// Where a turned bolt goes.
#[derive(Debug, Clone, Copy)]
pub enum Aim {
    /// Toward where the player looks (`forward`, its view), strayed by
    /// degrees: less while backing away (`back`) with the meter full
    /// enough.
    Crosshair { forward: Vec3, back: bool },
    /// Back at the shooter's head (a player with block and attack held),
    /// or its chest (a creature's).
    Shooter { head: bool },
    /// A blade not blocking: toward the shooter (a creature's) or the
    /// crosshair (a player's), wildly.
    Wild { way: Vec3 },
}

/// A way within `least..=most` degrees either side of `degrees`.
fn stray(rng: &mut SimRng, degrees: f32, [least, most]: [f32; 2]) -> f32 {
    let by = rng.range(least, most);
    if rng.below(2) == 0 {
        degrees - by
    } else {
        degrees + by
    }
}

/// `way` nudged by up to `spread` on each axis, made unit again.
fn spread(rng: &mut SimRng, mut way: Vec3, spread: f32) -> Vec3 {
    for component in &mut way {
        *component += rng.range(-spread, spread);
    }
    math::normalize(&mut way);
    way
}

/// The way a bolt turned at `at` goes (`docs/SABER-RULES.md` 7.3, 7.4),
/// its shooter at `shooter`, the blocker's meter `meter`, its blade idle
/// or not (`swinging`).
pub fn aim(
    aim: Aim,
    (at, shooter): (Vec3, Vec3),
    meter: i32,
    swinging: bool,
    rng: &mut SimRng,
) -> Vec3 {
    let [spent, full] = bolts::BANDS_SCATTER;
    match aim {
        Aim::Crosshair { forward, back } => {
            let [least, most, low_least, low_most] = bolts::SCATTER;
            let range = match () {
                _ if back && meter >= full => [0.0, bolts::BACK_SCATTER],
                _ if meter < spent => [low_least, low_most],
                _ => [least, most],
            };
            let (pitch, yaw) = crate::combat::mind::aim([0.0; 3], forward);
            let turned = [stray(rng, pitch, range), stray(rng, yaw, range), 0.0];
            math::angle_vectors(turned).0
        }
        Aim::Shooter { head } => {
            let [side, below, above] = bolts::HEAD;
            let mut target = shooter;
            if head {
                let (_, yaw) = crate::combat::mind::aim(at, shooter);
                let (_, right, _) = math::angle_vectors([0.0, yaw, 0.0]);
                target = math::mad(target, rng.range(-side, side), right);
                target[2] += bolts::HEAD_HEIGHT + rng.range(-below, above);
            } else {
                target[2] += bolts::HEAD_HEIGHT * bolts::CHEST;
            }
            let mut way = math::sub(target, at);
            math::normalize(&mut way);
            let [plain, wide] = bolts::AIMED_SPREAD;
            let by = match () {
                _ if meter < spent => wide,
                _ if swinging || meter < bolts::AIMED_FROM => plain,
                _ => return way,
            };
            spread(rng, way, by)
        }
        Aim::Wild { way } => spread(rng, way, bolts::WILD_SPREAD),
    }
}
