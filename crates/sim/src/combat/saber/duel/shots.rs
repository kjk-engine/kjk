//! Bolts against the blade under a duel mode (`docs/SABER-RULES.md` 7):
//! what turning one costs and where it goes. Who blocks is the world's
//! (`world::sabers::bolt_block`).

use crate::math::{self, Vec3};
use crate::rng::SimRng;
use rules::duel::bolts;

use super::SaberRules;

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
    rules: SaberRules,
    aim: Aim,
    (at, shooter): (Vec3, Vec3),
    meter: i32,
    swinging: bool,
    rng: &mut SimRng,
) -> Vec3 {
    let [spent, full] = bolts::BANDS_SCATTER;
    match aim {
        Aim::Crosshair { forward, back } => {
            let [md_least, md_most, mda_least, mda_most, low_least, low_most] = bolts::SCATTER;
            let range = match () {
                _ if back && (!rules.own_meter() || meter >= full) => [0.0, bolts::BACK_SCATTER],
                _ if !rules.own_meter() => [md_least, md_most],
                _ if meter < spent => [low_least, low_most],
                _ => [mda_least, mda_most],
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
            let [md_from, mda_from] = bolts::AIMED_FROM;
            let by = match () {
                _ if rules.own_meter() && meter < spent => wide,
                _ if !rules.own_meter() && swinging => wide,
                _ if swinging || meter < rules.of([md_from, mda_from]) => plain,
                _ => return way,
            };
            spread(rng, way, by)
        }
        Aim::Wild { way } => spread(rng, way, bolts::WILD_SPREAD),
    }
}
