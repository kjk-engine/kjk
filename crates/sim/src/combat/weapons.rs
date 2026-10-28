//! JA's blasters as KOTOR's ranged weapons fire them (`docs/COMBAT.md`),
//! and the weapon step JA's player movement runs after each move.
//!
//! Primary fire here; the alternate button takes a KOTOR feat special
//! shaped as JA's alternate for the gun (the Bryar's charge, the E-11's
//! fast fire, the disruptor's zoom: `combat::specials`).

use crate::combat::sheet::Blaster;

/// How one weapon's primary fire behaves (JA's table).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fire {
    /// Milliseconds between shots: also the attack's cadence for the pace
    /// factor (I22 C2).
    pub refire_ms: u32,
    /// Units per second; `None` for a hitscan (the disruptor).
    pub speed: Option<f32>,
    /// A random turn of up to this many degrees on pitch and on yaw, on
    /// every shard but the first.
    pub spread: f32,
    /// Shards per shot (the flechette's five).
    pub shards: u8,
    /// The bolt's box: this many units either side of its point.
    pub half_size: f32,
    /// Whether a saber can block, deflect or reflect it.
    pub blockable: bool,
    /// Where it leaves the gun: units forward, right and up of the eye
    /// (`CalcMuzzlePoint`).
    pub muzzle: [f32; 3],
}

/// The primary fire of each blaster (rulebook I23 table 1.2; the book's
/// `rules::weapons`).
pub fn primary(blaster: Blaster) -> Fire {
    use rules::weapons::bolt;
    let bolt = |refire_ms, speed| Fire {
        refire_ms,
        speed: Some(speed),
        spread: bolt::SPREAD,
        shards: bolt::SHARDS,
        half_size: bolt::HALF_SIZE,
        blockable: bolt::BLOCKABLE,
        muzzle: bolt::MUZZLE,
    };
    use rules::weapons::bowcaster;
    use rules::weapons::disruptor;
    use rules::weapons::ion;
    use rules::weapons::pistol;
    use rules::weapons::repeater;
    use rules::weapons::rifle;
    use rules::weapons::sonic;
    match blaster {
        // Bryar pistol.
        Blaster::Pistol => bolt(pistol::REFIRE_MS, pistol::SPEED),
        // E-11.
        Blaster::Rifle => bolt(rifle::REFIRE_MS, rifle::SPEED),
        // The repeater's primary sprays.
        Blaster::Repeater => Fire {
            spread: repeater::SPREAD,
            muzzle: repeater::MUZZLE,
            ..bolt(repeater::REFIRE_MS, repeater::SPEED)
        },
        // The bowcaster's tap: one wide bolt.
        Blaster::Bowcaster => Fire {
            half_size: bowcaster::HALF_SIZE,
            muzzle: bowcaster::MUZZLE,
            ..bolt(bowcaster::REFIRE_MS, bowcaster::SPEED)
        },
        // The disruptor's primary: a hitscan from the eye.
        Blaster::Disruptor => Fire {
            speed: None,
            muzzle: disruptor::MUZZLE,
            blockable: disruptor::BLOCKABLE,
            ..bolt(disruptor::REFIRE_MS, 0.0)
        },
        // DEMP2: a wide bolt, not blockable (it passes through blades).
        Blaster::Ion => Fire {
            half_size: ion::HALF_SIZE,
            blockable: ion::BLOCKABLE,
            ..bolt(ion::REFIRE_MS, ion::SPEED)
        },
        // Flechette: shards, the first exact, the rest spread.
        Blaster::Sonic => Fire {
            shards: sonic::SHARDS,
            half_size: sonic::HALF_SIZE,
            spread: sonic::SPREAD,
            ..bolt(sonic::REFIRE_MS, sonic::SPEED)
        },
    }
}

/// How far a hitscan reaches (units).
pub const HITSCAN_RANGE: f32 = rules::weapons::disruptor::RANGE;

/// How long a bolt flies before it goes, unseen (ms).
pub const BOLT_LIFE_MS: u64 = rules::weapons::flight::LIFE_MS;

/// A weapon's timer (JA's weapon cadence), in two halves so a special
/// can act between them (`combat::specials::pull`): each step counts down
/// by the step; while the trigger is held and it has run out, a shot fires
/// and the refire is added (the remainder carries over, so the cadence is
/// exact on average); let go, the timer clears once run out.
///
/// The countdown: whether the timer has run out.
pub fn count_down(timer_ms: &mut i32, msec: u32) -> bool {
    if *timer_ms > 0 {
        *timer_ms -= msec as i32;
    }
    *timer_ms <= 0
}

/// The trigger, once the timer is `ready`: whether a shot fires.
pub fn trigger(timer_ms: &mut i32, ready: bool, held: bool, refire_ms: u32) -> bool {
    if !ready {
        return false;
    }
    if !held {
        *timer_ms = 0;
        return false;
    }
    *timer_ms += refire_ms as i32;
    true
}
