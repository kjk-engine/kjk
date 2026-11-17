//! The probes of TSL's Force Deflection and Force Redirection (`kjk sim
//! probes --only deflection`; `combat::force::deflection`). Neither power
//! is cast: the force button cannot select either one, and neither counts
//! as a rank of Force Push. A gunner shoots at an empty-handed player of
//! the `m6` trace (`duels::play`) under JA's saber rules and under MDA.
//! Without either power the player turns no bolt. With Force Deflection
//! the player turns bolts and loses less health. With Force Redirection
//! the player turns bolts back, and they hurt the gunner. A Jedi with a lit
//! lightsaber who is stunned, held by a whirlwind or knocked down deflects
//! no bolt under either rule set (`Fighter::can_defend`).

use super::duel_probes::{count_events, fighters, shot_in};
use super::duels::play;
use crate::combat::Fighter;
use crate::combat::fighter::Outcome;
use crate::combat::force;
use crate::combat::sheet::Rows;
use crate::combat::spells::{FORCE_POWER_FORCE_REDIRECTION, FORCE_POWER_FORCE_REPULSION};
use crate::probe::{Checks, check};

/// The health `ticks` of one fighter show it lost.
fn lost(ticks: &[Fighter]) -> i32 {
    ticks
        .first()
        .zip(ticks.last())
        .map_or(0, |(a, b)| a.hp - b.hp)
}

/// What a pistol's shots did to an empty-handed player who knows the
/// `spells.2da` rows `powers`, under `rules`: the bolts the player turned,
/// the health the player lost and the health the gunner lost.
struct Shot {
    turned: usize,
    lost: i32,
    gunner_lost: i32,
}

fn shot_at(rules: &str, powers: &str) -> Result<Shot, String> {
    let powers = if powers.is_empty() {
        String::new()
    } else {
        format!(" powers={powers}")
    };
    let text = format!(
        "scenario probe\nrules {rules}\nseed 7\n\
         fighter 0 pistol x=0 hp=400\n\
         fighter 1 fists x=200 yaw=180 defence=0 hp=400{powers}\n\
         begin\ntick 60\ntick 600 b0=attack\n"
    );
    let ticks = play(&text)?;
    let (gunner, target) = (fighters(&ticks, 0), fighters(&ticks, 1));
    Ok(Shot {
        turned: count_events(&target, Outcome::Deflected),
        lost: lost(&target),
        gunner_lost: lost(&gunner),
    })
}

/// Neither power is cast: neither maps to a JA power, so the force button
/// selects neither, and a character who knows both has no Force Push nor
/// Force Pull to cast.
fn never_cast() -> Result<String, String> {
    let mut powers = Rows::<5>::default();
    powers.insert(FORCE_POWER_FORCE_REPULSION);
    powers.insert(FORCE_POWER_FORCE_REDIRECTION);
    let rows = [FORCE_POWER_FORCE_REPULSION, FORCE_POWER_FORCE_REDIRECTION];
    let mapped: Vec<_> = rows.iter().map(|row| force::of_row(*row)).collect();
    let choices = force::choices(&powers, None);
    check(
        mapped.iter().all(Option::is_none) && choices.is_empty(),
        format!("the rows cast as {mapped:?}; the force button selects {choices:?}"),
    )
}

/// Under `rules`: no power turns nothing; Force Deflection turns bolts and
/// spares health; Force Redirection turns bolts back into the gunner.
fn turns(rules: &str) -> Result<String, String> {
    let plain = shot_at(rules, "")?;
    let deflection = shot_at(rules, &FORCE_POWER_FORCE_REPULSION.to_string())?;
    let redirection = shot_at(
        rules,
        &format!("{FORCE_POWER_FORCE_REPULSION},{FORCE_POWER_FORCE_REDIRECTION}"),
    )?;
    check(
        plain.turned == 0
            && plain.lost > 0
            && deflection.turned > 0
            && deflection.lost < plain.lost
            && redirection.turned > 0
            && redirection.lost < plain.lost
            && redirection.gunner_lost > plain.gunner_lost,
        format!(
            "{rules}: without a power {} turned, {} health lost; with Force Deflection {} \
             turned, {} lost, the gunner {} lost; with Force Redirection {} turned, {} lost, \
             the gunner {} lost",
            plain.turned,
            plain.lost,
            deflection.turned,
            deflection.lost,
            deflection.gunner_lost,
            redirection.turned,
            redirection.lost,
            redirection.gunner_lost
        ),
    )
}

/// The ways a Jedi is held in `held_turns_nothing`, as `force` directive
/// keys: none, stunned, held by a whirlwind and knocked down.
const HOLDS: [&str; 4] = ["", "stunned=100000", "whirlwind=100000", "knockdown=100000"];

/// Under `rules`, a pistol shoots a player and a creature who face it with
/// a lit lightsaber and all three tiers of Jedi Defense. While free, each
/// deflects bolts. While stunned, held by a whirlwind or knocked down, each
/// deflects and reflects nothing, and every bolt fired lands on it.
fn held_turns_nothing(rules: &str) -> Result<String, String> {
    let block = if rules == "ja" { "" } else { "h1=block" };
    let mut right = true;
    let mut said = Vec::new();
    for (target, hold) in [("fighter 1 medium", block), ("creature medium", "")] {
        for held in HOLDS {
            let shots = shot_in(rules, target, 3, hold, 0.0, held)?;
            let did = if held.is_empty() {
                shots.deflected > 0
            } else {
                shots.fired > 0 && shots.deflected == 0 && shots.hit == shots.fired
            };
            right &= did;
            let held = if held.is_empty() { "free" } else { held };
            said.push(format!(
                "{target} {held}: of {} bolts {} deflected and {} hit",
                shots.fired, shots.deflected, shots.hit
            ));
        }
    }
    check(right, format!("{rules}: {}", said.join("; ")))
}

pub fn probes() -> Checks {
    vec![
        ("deflection_never_cast", never_cast()),
        ("deflection_ja", turns("ja")),
        ("deflection_mda", turns("mda")),
        ("held_jedi_deflects_nothing_ja", held_turns_nothing("ja")),
        (
            "held_jedi_deflects_nothing_advanced",
            held_turns_nothing("advanced"),
        ),
    ]
}
