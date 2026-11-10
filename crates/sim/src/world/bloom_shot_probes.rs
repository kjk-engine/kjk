//! The probes of a player's shots under bloom and kick (`kjk sim probes
//! --only "bloom shots"`, `combat::bloom`), on the proving ground, no
//! install:
//! - the crosshair doesn't lie: every bolt of a held rifle and a held
//!   flechette strays from the kicked aim line by no more than the spread
//!   the crosshair showed for it (`bloom::shot_cone`), and the spread grows
//!   as the trigger stays held;
//! - two pistols, firing in turn, bloom together: the twelfth shot's
//!   spread is wider than the first's;
//! - two pistols firing at once (a Power Blast's bolt from each gun on one
//!   step) both stay within the spread shown for that step;
//! - a gun put away mid-bloom comes back true: the first shot after it is
//!   drawn again shows no bloom;
//! - a companion leading the party (`world::lent`, a portrait click) shoots
//!   as its player: to the crosshair, blooming, within the spread shown,
//!   never with a creature's wandering aim.

use super::{Fighting, SimWorld};
use crate::combat::bloom::{kick_at, shot_cone};
use crate::combat::sheet::{Blaster, Delivery, feats};
use crate::combat::specials::Shooting;
use crate::combat::{Sheath, Sheet, mind};
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
        ("two_pistols_at_once_within_spread", at_once()),
        ("holstered_gun_comes_back_true", holstered()),
        ("led_companion_shoots_as_its_player", led()),
    ]
}

/// A bolt as it left: when (sim ms), the spread shown for it and how far it
/// strayed.
#[derive(Debug, Clone, Copy)]
struct Shot {
    ms: u64,
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

fn pressing(buttons: u16) -> Command {
    Command {
        angles: crate::moves::view_angles(2.0, 0.0),
        buttons,
        ..Command::default()
    }
}

/// A player on the proving ground with `blaster`, its bolts kept with the
/// spread shown the step before each left.
struct Range {
    world: SimWorld,
    blaster: Blaster,
    before: Option<(MotionState, Shooting)>,
    shots: Vec<Shot>,
}

impl Range {
    fn new(sheet: Sheet, blaster: Blaster) -> Result<Range, String> {
        let level = crate::determinism::proving_ground();
        let config = WorldConfig {
            sheet,
            ..WorldConfig::one_seat()
        };
        let world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
        Ok(Range::on(world, blaster))
    }

    /// `world`'s seat 0 shooting `blaster` from the body it acts through.
    fn on(world: SimWorld, blaster: Blaster) -> Range {
        Range {
            world,
            blaster,
            before: None,
            shots: Vec::new(),
        }
    }

    /// `ms` of steps with `command`, or until `count` bolts in all.
    fn run(&mut self, ms: u32, command: Command, count: usize) -> Result<(), String> {
        for _ in 0..ms / MSEC {
            let tick = self.world.tick() + 1;
            let mut input = TickInput::default();
            input.insert(Seat(0), command);
            let snapshot = crate::step(&mut self.world, tick, &input, MSEC, StepReason::Authority);
            let me = snapshot.body(Seat(0)).ok_or("no player")?;
            let ps = me.player.ok_or("no body")?;
            let fighter = me.fighter.as_ref().ok_or("no fighter")?;
            if let Some((was, shooting)) = self.before {
                let now = snapshot.time_ms;
                let cone = shot_cone(&was, &shooting, self.blaster, now);
                let [pitch, yaw] = kick_at(&shooting, now);
                let view = [was.view[0] + pitch, was.view[1] + yaw, 0.0];
                let (forward, ..) = math::angle_vectors(view);
                let from = mind::crosshair(&was);
                let far = math::mad(from, crate::combat::weapons::HITSCAN_RANGE, forward);
                let level = self.world.level();
                let states = level.states_in(&snapshot);
                let met = level.with(states.as_deref(), |level| {
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
                    self.shots.push(Shot { ms: now, cone, off });
                }
            }
            self.before = Some((ps, fighter.shooting));
            if self.shots.len() >= count {
                break;
            }
        }
        Ok(())
    }

    /// The player's gun put away (or drawn again).
    fn sheath(&mut self, sheath: Sheath) {
        let mut query = self.world.world.query::<&mut Fighting>();
        for mut fighting in query.iter_mut(&mut self.world.world) {
            fighting.0.sheath = sheath;
        }
    }

    /// The bolts that strayed past the spread shown for them.
    fn strays(&self) -> Vec<&Shot> {
        self.shots
            .iter()
            .filter(|shot| shot.off > shot.cone + SLACK)
            .collect()
    }
}

/// Every bolt within its shown spread, and the spread wider by the end.
fn truthful(blaster: Blaster, pair: bool) -> Result<String, String> {
    let count = if pair { 12 } else { 30 };
    let mut range = Range::new(armed(blaster, pair), blaster)?;
    range.run(30_000, pressing(buttons::ATTACK), count)?;
    let shots = &range.shots;
    if shots.len() < count {
        return Err(format!("{} bolts of {count} fired", shots.len()));
    }
    let strays = range.strays();
    let (first, last) = (shots[0].cone, shots[shots.len() - 1].cone);
    check(
        strays.is_empty() && last > first,
        format!("{blaster:?}: spread {first:.2}° → {last:.2}°; strays past it: {strays:?}"),
    )
}

/// The second companion leading the party with its pair of pistols drawn,
/// seat 0 holding the trigger: its bolts go to the crosshair within the
/// spread shown, and the spread grows.
fn led() -> Result<String, String> {
    use super::drivers::probes::{party_world, poke, run, scripts_of};
    let (mut world, _, [_, led]) = party_world(super::Guests::Avatars, 1)?;
    scripts_of(&mut world).state.party.leader = Some(1);
    poke(&mut world, led, |fighter, _| fighter.sheath = Sheath::Fight);
    let first = run(&mut world, &[(0, 0)]);
    let body = first.body(Seat(0)).map(|entity| entity.id);
    let mut sheets = world.world.query::<(
        &super::Id,
        Option<&super::PlayerSheet>,
        Option<&super::Object>,
    )>();
    let delivery = sheets
        .iter(&world.world)
        .find(|(id, ..)| id.0 == led)
        .and_then(|(_, sheet, object)| super::sheet_of(sheet, object).weapons[0])
        .map(|weapon| weapon.delivery);
    let Some(Delivery::Blaster(blaster)) = delivery else {
        return Err(format!("the companion holds {delivery:?}"));
    };
    let mut range = Range::on(world, blaster);
    range.run(30_000, pressing(buttons::ATTACK), 12)?;
    let shots = &range.shots;
    let strays = range.strays();
    let (first, last) = (
        shots.first().map(|shot| shot.cone),
        shots.last().map(|shot| shot.cone),
    );
    check(
        body == Some(led) && shots.len() >= 12 && strays.is_empty() && last > first,
        format!(
            "seat 0 acts through {body:?} (led {led:?}), {} bolts, spread {first:?} → {last:?}; strays past it: {strays:?}",
            shots.len()
        ),
    )
}

/// A pistol pair's Power Blast from rest: a bolt from each gun on one step,
/// both within the spread shown for it.
fn at_once() -> Result<String, String> {
    let mut sheet = armed(Blaster::Pistol, true);
    sheet.feats = sheet.feats.with(feats::POWER_BLAST[0]);
    let mut range = Range::new(sheet, Blaster::Pistol)?;
    range.run(200, pressing(0), usize::MAX)?;
    range.run(600, pressing(buttons::ALT_ATTACK), usize::MAX)?;
    range.run(200, pressing(0), usize::MAX)?;
    let together = matches!(&range.shots[..], [a, b] if a.ms == b.ms);
    let strays = range.strays();
    check(
        together && strays.is_empty(),
        format!(
            "bolts {:?}; strays past the spread: {strays:?}",
            range.shots
        ),
    )
}

/// A pistol bloomed, put away and drawn again within its rest: its first
/// shot after shows no bloom.
fn holstered() -> Result<String, String> {
    let mut range = Range::new(armed(Blaster::Pistol, false), Blaster::Pistol)?;
    range.run(3000, pressing(buttons::ATTACK), usize::MAX)?;
    let bloomed = range.shots.last().map(|shot| shot.cone);
    range.sheath(Sheath::Holstered);
    range.run(200, pressing(0), usize::MAX)?;
    range.sheath(Sheath::Fight);
    let before = range.shots.len();
    range.run(1000, pressing(buttons::ATTACK), before + 1)?;
    let after = range.shots.get(before).map(|shot| shot.cone);
    check(
        bloomed.is_some_and(|cone| cone > 0.0) && after == Some(0.0),
        format!("bloomed {bloomed:?}, first shot drawn again {after:?}"),
    )
}
