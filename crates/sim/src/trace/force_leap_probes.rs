//! The probes of the Force leap (`kjk sim probes --only "force leap"`): a
//! player with Force Jump who starts an ordinary swing and presses [Jump]
//! within the window leaps at the enemy in front and strikes once.
//! - With Force Jump, the swing and the press, the player leaps and lands
//!   beside the enemy, who stood beyond the least distance, and stays there.
//! - An enemy a little to one side, within the aim's arc, is leapt at too;
//!   one well to the side is not.
//! - Without Force Jump, the same keys make no leap.
//! - Without the swing, the press makes no leap.
//! - With the enemy nearer than the least distance, no leap is made.
//! - After one leap, the same keys from as far away leap again.
//!
//! Each is played from a trace scenario (`trace::duels`) under Jedi
//! Academy's rules, against a hostile creature put in front of the player.
use super::duels::play;
use crate::Seat;
use crate::math::{self, Vec3};
use crate::module::ObjectKind;
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, Snapshot};
use rules::saber_moves::specials;
/// How far along the arena's y axis (units) the fight stands, clear of its
/// steps and its block.
const LANE: f32 = 250.0;
/// A body's origin height over the arena's floor (units).
const STANDING_Z: f32 = 24.125;
/// The ticks before the swing: the settling and the blade's readying.
const READY: usize = 211;
/// How near (units, centre to centre) the leap must bring the player.
const LANDED: f32 = 140.0;
/// What a fight with the creature `away` units in front did: whether the
/// player leapt, and the nearest it came to the creature afterwards.
fn fight(jump: u8, away: f32, swing: bool) -> Result<(bool, f32, f32), String> {
    fight_at(jump, away, 0.0, swing)
}
/// `fight`, the creature `side` degrees to the left of straight ahead. What
/// it did: whether the player leapt, the nearest it came to the creature,
/// and how far apart they stood at the end.
fn fight_at(jump: u8, away: f32, side: f32, swing: bool) -> Result<(bool, f32, f32), String> {
    let swing = if swing { "b0=attack " } else { "" };
    let (sin, cos) = collision::trig::sin_cos_deg(side);
    let (x, y) = (away * cos, LANE + away * sin);
    let text = format!(
        "scenario force leap\nrules ja\nseed 7\n\
         fighter 0 medium x=0 y={LANE} jump={jump} defence=2 tiers=2 fp=100 hp=400\n\
         creature medium x=1400 y={LANE} yaw=180 defence=2 tiers=2 hp=4000\n\
         begin\ntick 1 b0=attack\ntick {}\n\
         put c0 {x} {y} {STANDING_Z} 180\n\
         tick 1 {swing}\ntick 5\ntick 1 u0=127\ntick 220\n",
        READY - 1,
    );
    let ticks: Vec<Snapshot> = play(&text)?;
    let after = &ticks[ticks.len().saturating_sub(230)..];
    let leapt = after
        .iter()
        .filter_map(|tick| tick.own(Seat(0)).and_then(|entity| entity.fighter))
        .any(|fighter| fighter.leap.is_some());
    let nearest = after
        .iter()
        .filter_map(|tick| Some(math::flat_distance(player(tick)?, creature(tick)?)))
        .fold(f32::MAX, f32::min);
    let last = after
        .last()
        .and_then(|tick| Some(math::flat_distance(player(tick)?, creature(tick)?)))
        .unwrap_or(f32::MAX);
    Ok((leapt, nearest, last))
}
fn player(tick: &Snapshot) -> Option<Vec3> {
    tick.own(Seat(0))?.player.map(|ps| ps.origin)
}
fn creature(tick: &Snapshot) -> Option<Vec3> {
    tick.entities
        .iter()
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, 0))
        .and_then(|entity| entity.player)
        .map(|ps| ps.origin)
}
/// Far enough for a leap of each rank, near enough for the first.
fn far() -> f32 {
    (specials::LEAP_LEAST + specials::LEAP_MOST[0]) / 2.0
}
fn leaps() -> Result<String, String> {
    let away = far();
    let (leapt, nearest, last) = fight(1, away, true)?;
    check(
        leapt && nearest <= LANDED && last <= LANDED,
        format!(
            "enemy {away} units off: leapt {leapt}, came within {nearest:.0} units, \
             {last:.0} units apart at the end"
        ),
    )
}
fn in_the_arc() -> Result<String, String> {
    let (away, side) = (far(), specials::LEAP_ARC * 0.6);
    let (leapt, nearest, _) = fight_at(1, away, side, true)?;
    check(
        leapt && nearest <= LANDED,
        format!(
            "enemy {away} units off, {side} degrees aside: leapt {leapt}, came within {nearest:.0}"
        ),
    )
}
fn out_of_the_arc() -> Result<String, String> {
    let (away, side) = (far(), specials::LEAP_ARC * 2.0);
    let (leapt, _, _) = fight_at(1, away, side, true)?;
    check(
        !leapt,
        format!("enemy {away} units off, {side} degrees aside: leapt {leapt}"),
    )
}
fn no_feat() -> Result<String, String> {
    let away = far();
    let (leapt, _, _) = fight(0, away, true)?;
    check(
        !leapt,
        format!("without Force Jump, enemy {away} units off: leapt {leapt}"),
    )
}
fn no_swing() -> Result<String, String> {
    let away = far();
    let (leapt, _, _) = fight(1, away, false)?;
    check(
        !leapt,
        format!("[Jump] without a swing, enemy {away} units off: leapt {leapt}"),
    )
}
fn too_near() -> Result<String, String> {
    let away = specials::LEAP_LEAST - 100.0;
    let (leapt, _, _) = fight(1, away, true)?;
    check(!leapt, format!("enemy {away} units off: leapt {leapt}"))
}
pub fn probes() -> Checks {
    vec![
        ("the player leaps at a distant enemy", leaps()),
        ("the leap goes for an enemy a little aside", in_the_arc()),
        ("no leap at an enemy well aside", out_of_the_arc()),
        ("the player leaps again after the first leap", leaps_again()),
        ("the player leaps among several enemies", among_several()),
        ("the player leaps out of a melee", out_of_a_melee()),
        (
            "the player leaps out of a melee with [Attack] held",
            out_of_a_held_melee(),
        ),
        ("no leap without Force Jump", no_feat()),
        ("no leap without a swing", no_swing()),
        ("no leap at a near enemy", too_near()),
    ]
}
/// A second leap after the first: the player leaps, both are put back
/// apart as they started, and the same keys leap again.
fn leaps_again() -> Result<String, String> {
    let away = far();
    let text = format!(
        "scenario force leap twice\nrules ja\nseed 7\n\
         fighter 0 medium x=0 y={LANE} jump=1 defence=2 tiers=2 fp=100 hp=400\n\
         creature medium x=1400 y={LANE} yaw=180 defence=2 tiers=2 hp=4000\n\
         begin\ntick 1 b0=attack\ntick {}\n\
         put c0 {away} {LANE} {STANDING_Z} 180\n\
         tick 1 b0=attack\ntick 5\ntick 1 u0=127\ntick 220\n\
         put 0 0 {LANE} {STANDING_Z} 0\nput c0 {away} {LANE} {STANDING_Z} 180\n\
         tick 60\ntick 1 b0=attack\ntick 5\ntick 1 u0=127\ntick 220\n",
        READY - 1,
    );
    let ticks: Vec<Snapshot> = play(&text)?;
    let leaping: Vec<bool> = ticks
        .iter()
        .map(|tick| {
            tick.own(Seat(0))
                .and_then(|entity| entity.fighter)
                .is_some_and(|fighter| fighter.leap.is_some())
        })
        .collect();
    let starts = leaping.windows(2).filter(|w| !w[0] && w[1]).count();
    check(
        starts >= 2,
        format!("enemy {away} units off twice: {starts} leaps started"),
    )
}
/// Three enemies: one straight ahead far off, one nearer a little aside in
/// the way, one well to the side. The leap goes for one of them.
fn among_several() -> Result<String, String> {
    let far = far();
    let text = format!(
        "scenario force leap among several\nrules ja\nseed 7\n\
         fighter 0 medium x=0 y={LANE} jump=1 defence=2 tiers=2 fp=100 hp=400\n\
         creature medium x=1400 y={LANE} yaw=180 defence=2 tiers=2 hp=4000\n\
         creature medium x=1400 y={} yaw=180 defence=2 tiers=2 hp=4000\n\
         creature medium x=1400 y={} yaw=180 defence=2 tiers=2 hp=4000\n\
         begin\ntick 1 b0=attack\ntick {}\n\
         put c0 {far} {LANE} {STANDING_Z} 180\n\
         put c1 {} {} {STANDING_Z} 180\n\
         put c2 {} {} {STANDING_Z} 180\n\
         tick 1 b0=attack\ntick 5\ntick 1 u0=127\ntick 220\n",
        LANE + 200.0,
        LANE - 200.0,
        READY - 1,
        far * 0.6,
        LANE + 20.0,
        far * 0.5,
        LANE - 300.0,
    );
    let ticks: Vec<Snapshot> = play(&text)?;
    let leapt = ticks
        .iter()
        .filter_map(|tick| tick.own(Seat(0)).and_then(|entity| entity.fighter))
        .find_map(|fighter| fighter.leap);
    check(
        leapt.is_some(),
        format!(
            "three enemies, the nearest {} units off: leap {leapt:?}",
            far * 0.5
        ),
    )
}
/// An enemy in melee range, fighting the player and meeting its swing, and
/// another farther off a little aside: the swing and the press leap at the
/// one farther off.
fn out_of_a_melee() -> Result<String, String> {
    melee("tick 120\ntick 1 b0=attack\ntick 5\ntick 1 u0=127\ntick 220\n")
}
/// `out_of_a_melee`, [Attack] held all along, so the same swing repeats.
fn out_of_a_held_melee() -> Result<String, String> {
    melee("tick 120 b0=attack\ntick 1 u0=127 b0=attack\ntick 220\n")
}
fn melee(keys: &str) -> Result<String, String> {
    let far = far();
    let (sin, cos) = collision::trig::sin_cos_deg(specials::LEAP_ARC * 0.6);
    let text = format!(
        "scenario force leap out of a melee\nrules ja\nseed 7\n\
         fighter 0 medium x=0 y={LANE} jump=1 defence=2 tiers=2 fp=100 hp=4000\n\
         creature medium x=60 y={LANE} yaw=180 defence=3 tiers=2 hp=4000\n\
         creature medium x=1400 y={} yaw=180 defence=2 tiers=2 hp=4000\n\
         begin\ntick 1 b0=attack\ntick {}\n\
         put c1 {} {} {STANDING_Z} 180\n{keys}",
        LANE + 300.0,
        READY - 1,
        far * cos,
        LANE + far * sin,
    );
    let ticks: Vec<Snapshot> = play(&text)?;
    let leapt = ticks
        .iter()
        .filter_map(|tick| tick.own(Seat(0)).and_then(|entity| entity.fighter))
        .find_map(|fighter| fighter.leap);
    let moves: Vec<String> = ticks
        .iter()
        .rev()
        .skip(220)
        .take(125)
        .step_by(8)
        .filter_map(|tick| tick.own(Seat(0)).and_then(|entity| entity.fighter))
        .map(|fighter| {
            format!(
                "{} armed {}",
                fighter.saber.current_move.name(),
                fighter.leap_armed_until
            )
        })
        .collect();
    check(
        leapt.is_some(),
        format!(
            "an enemy in melee, another {far} units off: leap {leapt:?}; before the press {moves:?}"
        ),
    )
}
