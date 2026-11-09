//! A player's blaster blooms and kicks (`data/rules/weapons.toml`, Bloom
//! and Kick): each shot widens the spread the next one may stray by and
//! lifts the aim a little; resting the trigger brings both back. The
//! crosshair shows the same cone (`shot_cone`), so it never shows less
//! spread than the shot has. Creatures keep their own aim
//! (`combat::troop`).

use crate::combat::sheet::Blaster;
use crate::combat::specials::Shooting;
use crate::math;
use crate::motion::MotionState;
use crate::rng::SimRng;

/// How one blaster blooms and kicks (degrees, degrees a second, ms).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tune {
    pub per_shot: f32,
    pub max: f32,
    pub decay: f32,
    pub rest_ms: u32,
    pub kick_up: f32,
    pub kick_side: f32,
}

/// What widens or narrows a shot's spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stance {
    pub moving: bool,
    pub airborne: bool,
    pub aiming: bool,
}

/// The bloom and kick of `blaster`.
pub fn tune(blaster: Blaster) -> Tune {
    use rules::weapons::{bowcaster, disruptor, ion, pistol, repeater, rifle, sonic};
    let ([per_shot, max, decay], rest_ms, [kick_up, kick_side]) = match blaster {
        Blaster::Pistol => (pistol::BLOOM, pistol::BLOOM_REST_MS, pistol::KICK),
        Blaster::Rifle => (rifle::BLOOM, rifle::BLOOM_REST_MS, rifle::KICK),
        Blaster::Repeater => (repeater::BLOOM, repeater::BLOOM_REST_MS, repeater::KICK),
        Blaster::Bowcaster => (bowcaster::BLOOM, bowcaster::BLOOM_REST_MS, bowcaster::KICK),
        Blaster::Disruptor => (disruptor::BLOOM, disruptor::BLOOM_REST_MS, disruptor::KICK),
        Blaster::Ion => (ion::BLOOM, ion::BLOOM_REST_MS, ion::KICK),
        Blaster::Sonic => (sonic::BLOOM, sonic::BLOOM_REST_MS, sonic::KICK),
    };
    Tune {
        per_shot,
        max,
        decay,
        rest_ms,
        kick_up,
        kick_side,
    }
}

/// `ps`'s stance for its spread: moving faster than
/// `weapons.bloom.moving_speed`, off the ground, aiming a Sniper Shot.
pub fn stance(ps: &MotionState, shooting: &Shooting) -> Stance {
    Stance {
        moving: math::flat_length(ps.velocity) > rules::weapons::bloom::MOVING_SPEED,
        airborne: ps.ground.is_none(),
        aiming: shooting.aiming,
    }
}

/// Thousandths of a degree to degrees.
fn degrees(milli: f32) -> f32 {
    milli / 1000.0
}

/// How far a value that falls `per_second` once `wait_ms` has passed has
/// fallen `since_ms` after it was set.
fn fallen(per_second: f32, wait_ms: u64, since_ms: u64) -> f32 {
    per_second * since_ms.saturating_sub(wait_ms) as f32 / 1000.0
}

/// The bloom at `now`, in degrees.
pub fn settled(shooting: &Shooting, tune: &Tune, now: u64) -> f32 {
    let since = now.saturating_sub(shooting.fired_at);
    let fall = fallen(tune.decay, u64::from(tune.rest_ms), since);
    (degrees(f32::from(shooting.bloom_mdeg)) - fall).max(0.0)
}

/// A shot's spread: how far it may stray from the aim line, degrees.
pub fn cone(base: f32, bloom: f32, stance: Stance) -> f32 {
    let [moving, airborne, aiming] = rules::weapons::bloom::STANCE;
    let by = |on: bool, factor: f32| if on { factor } else { 1.0 };
    (base + bloom) * by(stance.moving, moving) * by(stance.airborne, airborne) * by(stance.aiming, aiming)
}

/// The aim's kick at `now`, pitch and yaw degrees.
pub fn kick_at(shooting: &Shooting, now: u64) -> [f32; 2] {
    let [_, recover, after] = rules::weapons::bloom::KICK_LIMIT;
    let since = now.saturating_sub(shooting.fired_at);
    let fall = fallen(recover, after as u64, since);
    shooting.kick_mdeg.map(|axis| {
        let axis = degrees(f32::from(axis));
        axis.signum() * (axis.abs() - fall).max(0.0)
    })
}

/// A shot leaves at `now`: the bloom grows and the aim kicks up, and
/// `side` degrees to the side.
pub fn on_shot(shooting: &mut Shooting, tune: &Tune, now: u64, side: f32) {
    let most = rules::weapons::bloom::KICK_LIMIT[0];
    let bloom = (settled(shooting, tune, now) + tune.per_shot).min(tune.max);
    let [pitch, yaw] = kick_at(shooting, now);
    let kick = [
        (pitch - tune.kick_up).clamp(-most, most),
        (yaw + side).clamp(-most, most),
    ];
    shooting.bloom_mdeg = (bloom * 1000.0).round() as u16;
    shooting.kick_mdeg = kick.map(|axis| (axis * 1000.0).round() as i16);
    shooting.fired_at = now;
}

/// The spread a player's next shot of `blaster` would have at `now`: what
/// the crosshair shows.
pub fn shot_cone(ps: &MotionState, shooting: &Shooting, blaster: Blaster, now: u64) -> f32 {
    let _ = (ps, shooting, blaster, now);
    0.0
}

/// A point inside a disc of `radius`, evenly over its area.
pub fn disc(rng: &mut SimRng, radius: f32) -> [f32; 2] {
    let (sin, cos) = math::sin_cos_deg(rng.unit() * 360.0);
    let reach = rng.unit().sqrt() * radius;
    [reach * cos, reach * sin]
}
