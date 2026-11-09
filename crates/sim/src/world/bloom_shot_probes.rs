//! The probes of a player's shots under bloom and kick (`kjk sim probes
//! --only "bloom shots"`, `combat::bloom`), on the proving ground, no
//! install:
//! - the crosshair doesn't lie: every bolt of a held rifle and a held
//!   flechette strays from the kicked aim line by no more than the spread
//!   the crosshair showed for it (`bloom::shot_cone`), and the spread grows
//!   as the trigger stays held;
//! - two pistols, firing in turn, bloom together: the twelfth shot's
//!   spread is wider than the first's.

use super::SimWorld;
use crate::combat::bloom::{kick_at, shot_cone};
use crate::combat::sheet::{Blaster, Delivery};
use crate::combat::specials::Shooting;
use crate::combat::{Sheet, mind};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::math::{self, Vec3};
use crate::motion::MotionState;
use crate::probe::{Checks, check};
use crate::{StepReason, WorldConfig};
use collision::{Contents, Extents, UNIT};

const MSEC: u32 = 8;
/// How far a bolt may stray past its spread: the aim's rounding.
const SLACK: f32 = 0.05;

pub fn probes() -> Checks {
    vec![
        (
            "crosshair_does_not_lie_rifle",
            truthful(Blaster::Rifle, false),
        ),
        (
            "crosshair_does_not_lie_flechette",
            truthful(Blaster::Sonic, false),
        ),
        (
            "two_pistols_bloom_together",
            truthful(Blaster::Pistol, true),
        ),
    ]
}

/// A bolt as it left: the spread shown for it and how far it strayed.
#[derive(Debug, Clone, Copy)]
struct Shot {
    cone: f32,
    off: f32,
}

/// The dev soldier holding `blaster` (in both hands when `pair`).
fn armed(blaster: Blaster, pair: bool) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    if let Some(weapon) = sheet.weapons[0].as_mut() {
        weapon.delivery = Delivery::Blaster(blaster);
    }
    if pair {
        sheet.weapons[1] = sheet.weapons[0];
    }
    sheet
}

/// The angle between two ways (degrees).
fn between(mut a: Vec3, mut b: Vec3) -> f32 {
    math::normalize(&mut a);
    math::normalize(&mut b);
    math::dot(a, b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Holds the trigger until `count` bolts have left (or 30 s pass): each
/// bolt with the spread shown the step before it left.
fn hold(blaster: Blaster, pair: bool, count: usize) -> Result<Vec<Shot>, String> {
    let level = crate::determinism::proving_ground();
    let config = WorldConfig {
        sheet: armed(blaster, pair),
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
    let command = Command {
        angles: crate::moves::view_angles(2.0, 0.0),
        buttons: buttons::ATTACK,
        ..Command::default()
    };
    let mut before: Option<(MotionState, Shooting)> = None;
    let mut shots = Vec::new();
    for tick in 1..=30_000 / u64::from(MSEC) {
        let mut input = TickInput::default();
        input.insert(Seat(0), command);
        let snapshot = crate::step(&mut world, tick, &input, MSEC, StepReason::Authority);
        let me = snapshot.own(Seat(0)).ok_or("no player")?;
        let ps = me.player.ok_or("no body")?;
        let fighter = me.fighter.as_ref().ok_or("no fighter")?;
        if let Some((was, shooting)) = before {
            let now = snapshot.time_ms;
            let cone = shot_cone(&was, &shooting, blaster, now);
            let [pitch, yaw] = kick_at(&shooting, now);
            let view = [was.view[0] + pitch, was.view[1] + yaw, 0.0];
            let (forward, ..) = math::angle_vectors(view);
            let from = mind::crosshair(&was);
            let far = math::mad(from, crate::combat::weapons::HITSCAN_RANGE, forward);
            let states = world.level().states_in(&snapshot);
            let met = world.level().with(states.as_deref(), |level| {
                level.trace(
                    math::scale(from, UNIT),
                    math::scale(far, UNIT),
                    &Extents::POINT,
                    Contents::SOLID,
                )
            });
            let point = math::scale(met.end, 1.0 / UNIT);
            let fired = snapshot.entities.iter().filter_map(|entity| entity.missile);
            for missile in fired.filter(|m| m.owner == me.id && m.from_ms == now) {
                let off = between(missile.velocity, math::sub(point, missile.base));
                shots.push(Shot { cone, off });
            }
        }
        before = Some((ps, fighter.shooting));
        if shots.len() >= count {
            return Ok(shots);
        }
    }
    Err(format!("{} bolts of {count} fired", shots.len()))
}

/// Every bolt within its shown spread, and the spread wider by the end.
fn truthful(blaster: Blaster, pair: bool) -> Result<String, String> {
    let count = if pair { 12 } else { 30 };
    let shots = hold(blaster, pair, count)?;
    let strays: Vec<&Shot> = shots
        .iter()
        .filter(|shot| shot.off > shot.cone + SLACK)
        .collect();
    let (first, last) = (shots[0].cone, shots[shots.len() - 1].cone);
    check(
        strays.is_empty() && last > first,
        format!("{blaster:?}: spread {first:.2}° → {last:.2}°; strays past it: {strays:?}"),
    )
}
