//! The own blaster's crosshair in play (`ui::screens::hud::reticle`): its
//! shape by the gun in hand, its gap the spread the next shot may stray by
//! (`sim::combat::bloom::shot_cone`) through the camera's field of view.
//! The spread widens on the screen at once, so it never shows less than a
//! shot has, and narrows over `NARROW_MS`, so it doesn't twitch.

use sim::combat::Delivery;
use sim::combat::sheet::Blaster;
use sim::{EntityState, Instances};
use ui::screens::hud::Family;
use ui::vm::ReticleView;

/// How long the crosshair takes to narrow most of the way (ms).
pub const NARROW_MS: f32 = 60.0;

/// The crosshair's shape for `blaster`.
pub fn family_of(blaster: Blaster) -> Family {
    match blaster {
        Blaster::Pistol | Blaster::Rifle => Family::Cross,
        Blaster::Repeater => Family::Prongs,
        Blaster::Bowcaster => Family::Brackets,
        Blaster::Disruptor => Family::Lines,
        Blaster::Ion => Family::Ring,
        Blaster::Sonic => Family::Dashes,
    }
}

/// The spread shown `dt_ms` after `shown`, toward `target`: wider at once,
/// narrower by easing.
pub fn ease(shown: f32, target: f32, dt_ms: f32) -> f32 {
    if target >= shown {
        target
    } else {
        target + (shown - target) * (-dt_ms * 3.0 / NARROW_MS).exp()
    }
}

/// What the own crosshair shows of `own` (the own character at own time,
/// `own_ms`) through a camera of vertical field of view `fov` (degrees),
/// before easing. `None` without a blaster in hand, down, or aiming a
/// disruptor's Sniper Shot (its scope shows the dot alone).
pub fn of(own: &EntityState, objects: &Instances, own_ms: f64, fov: f32) -> Option<ReticleView> {
    let ps = own.player?;
    let shooting = own.fighter.filter(|fighter| fighter.alive())?.shooting;
    let Delivery::Blaster(blaster) = crate::cast::weapon(own, objects).delivery else {
        return None;
    };
    if blaster == Blaster::Disruptor && shooting.aiming {
        return None;
    }
    let now = own_ms.max(0.0) as u64;
    Some(ReticleView {
        family: family_of(blaster),
        cone: sim::combat::bloom::shot_cone(&ps, &shooting, blaster, now),
        fov,
    })
}

/// The probes (`kjk sim probes --only "reticle view"`): each blaster's
/// shape, and the gap widening at once and narrowing by easing.
pub fn probes() -> sim::probe::Checks {
    let shapes = [
        (Blaster::Pistol, Family::Cross),
        (Blaster::Rifle, Family::Cross),
        (Blaster::Repeater, Family::Prongs),
        (Blaster::Bowcaster, Family::Brackets),
        (Blaster::Disruptor, Family::Lines),
        (Blaster::Ion, Family::Ring),
        (Blaster::Sonic, Family::Dashes),
    ];
    let wrong: Vec<_> = shapes
        .iter()
        .filter(|(blaster, family)| family_of(*blaster) != *family)
        .collect();
    let widened = ease(1.0, 2.0, 16.0);
    let narrowing = ease(2.0, 1.0, 16.0);
    let settled = ease(2.0, 1.0, 1000.0);
    vec![
        (
            "family_by_blaster",
            sim::probe::check(wrong.is_empty(), format!("wrong shapes: {wrong:?}")),
        ),
        (
            "ease_widens_at_once",
            sim::probe::check(
                widened == 2.0
                    && narrowing > 1.0
                    && narrowing < 2.0
                    && (settled - 1.0).abs() < 1e-3,
                format!("1→2 in 16 ms: {widened}; 2→1 in 16 ms: {narrowing}, in 1 s: {settled}"),
            ),
        ),
    ]
}
