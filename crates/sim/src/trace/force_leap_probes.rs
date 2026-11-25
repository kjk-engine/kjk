//! The probes of the Force leap (`kjk sim probes --only "force leap"`): a
//! player with Force Jump who starts an ordinary swing and presses [Jump]
//! within the window leaps at the enemy in front and strikes once.
//! - With Force Jump, the swing and the press, the player leaps and lands
//!   beside the enemy, who stood beyond the least distance.
//! - Without Force Jump, the same keys make no leap.
//! - Without the swing, the press makes no leap.
//! - With the enemy nearer than the least distance, no leap is made.
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
fn fight(jump: u8, away: f32, swing: bool) -> Result<(bool, f32), String> {
    let swing = if swing { "b0=attack " } else { "" };
    let x = away;
    let text = format!(
        "scenario force leap\nrules ja\nseed 7\n\
         fighter 0 medium x=0 y={LANE} jump={jump} defence=2 tiers=2 fp=100 hp=400\n\
         creature medium x=1400 y={LANE} yaw=180 defence=2 tiers=2 hp=4000\n\
         begin\ntick 1 b0=attack\ntick {}\n\
         put c0 {x} {LANE} {STANDING_Z} 180\n\
         tick 1 {swing}c0=0\ntick 5 c0=0\ntick 1 u0=127 c0=0\ntick 220 c0=0\n",
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
    Ok((leapt, nearest))
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
    let (leapt, nearest) = fight(1, away, true)?;
    check(
        leapt && nearest <= LANDED,
        format!("enemy {away} units off: leapt {leapt}, came within {nearest:.0} units"),
    )
}
fn no_feat() -> Result<String, String> {
    let away = far();
    let (leapt, _) = fight(0, away, true)?;
    check(
        !leapt,
        format!("without Force Jump, enemy {away} units off: leapt {leapt}"),
    )
}
fn no_swing() -> Result<String, String> {
    let away = far();
    let (leapt, _) = fight(1, away, false)?;
    check(
        !leapt,
        format!("[Jump] without a swing, enemy {away} units off: leapt {leapt}"),
    )
}
fn too_near() -> Result<String, String> {
    let away = specials::LEAP_LEAST - 100.0;
    let (leapt, _) = fight(1, away, true)?;
    check(!leapt, format!("enemy {away} units off: leapt {leapt}"))
}
pub fn probes() -> Checks {
    vec![
        ("the player leaps at a distant enemy", leaps()),
        ("no leap without Force Jump", no_feat()),
        ("no leap without a swing", no_swing()),
        ("no leap at a near enemy", too_near()),
    ]
}
